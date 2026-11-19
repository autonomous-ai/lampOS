//! The A733's NPU through VeriSilicon's VIPLite (`libNBGlinker.so`, 2.0.3.2-AW
//! on lamp-0c89; `libVIPhal.so` lacks `vip_map_buffer` on the lamp image).
//! The library is opened at run time, so the build needs no vendor file and a
//! lamp without it is told "unsupported", never a crash. One network binary
//! graph (`.nb`) per [`VipNet`]; its input is one frame of unsigned bytes
//! (the model's normalisation is baked in when it is converted), its outputs
//! are read raw with their quantisation and decoded by a head on the CPU.
//!
//! This is the only place in lampOS that calls into the vendor's library,
//! and so the only unsafe code in the crate: every call is a C function with
//! the signature `vip_lite.h` gives, on handles this module made and keeps.
use crate::engine::{Backend, Format, Input, LoadError, Net, Quant, RunMs, Tensor};
use libloading::Library;
use std::{
    ffi::{CString, c_void},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

/// The library the lamp's image ships.
pub const LIBRARY: &str = "/usr/lib/libNBGlinker.so";
/// The NPU's device, opened by the library.
pub const DEVICE: &str = "/dev/vipcore";

// vip_lite.h's enums, as the reference binding uses them.
const CREATE_FROM_FILE: u32 = 1;
const NETWORK_INPUTS: u32 = 1;
const NETWORK_OUTPUTS: u32 = 2;
const PROP_QUANT_FORMAT: u32 = 0;
const PROP_NUM_DIMS: u32 = 1;
const PROP_SIZES: u32 = 2;
const PROP_DATA_FORMAT: u32 = 3;
const PROP_FIXED_POINT_POS: u32 = 4;
const PROP_SCALE: u32 = 5;
const PROP_ZERO_POINT: u32 = 6;
const QUANT_NONE: i32 = 0;
const QUANT_DFP: i32 = 1;
const QUANT_AFFINE: i32 = 2;
const FLUSH: u32 = 1;
const INVALIDATE: u32 = 2;
/// The most dimensions a VIPLite buffer has.
const MOST_DIMS: usize = 6;
/// The most outputs read from one network.
const MOST_OUTPUTS: u32 = 32;

#[repr(C)]
#[derive(Clone, Copy)]
struct Affine {
    scale: f32,
    zero_point: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
union QuantData {
    fixed_point_pos: i32,
    affine: Affine,
}

/// `vip_buffer_create_params_t`.
#[repr(C)]
struct BufferParams {
    num_of_dims: u32,
    sizes: [u32; MOST_DIMS],
    data_format: i32,
    quant_format: i32,
    quant_data: QuantData,
    memory_type: u32,
}

type Handle = *mut c_void;
type Status = i32;

/// The functions used, looked up once.
struct Api {
    init: unsafe extern "C" fn() -> Status,
    destroy: unsafe extern "C" fn() -> Status,
    create_network: unsafe extern "C" fn(*const c_void, u32, u32, *mut Handle) -> Status,
    query_network: unsafe extern "C" fn(Handle, u32, *mut c_void) -> Status,
    query_input: unsafe extern "C" fn(Handle, u32, u32, *mut c_void) -> Status,
    query_output: unsafe extern "C" fn(Handle, u32, u32, *mut c_void) -> Status,
    create_buffer: unsafe extern "C" fn(*const BufferParams, u32, *mut Handle) -> Status,
    map_buffer: unsafe extern "C" fn(Handle) -> *mut c_void,
    unmap_buffer: unsafe extern "C" fn(Handle) -> Status,
    destroy_buffer: unsafe extern "C" fn(Handle) -> Status,
    flush_buffer: unsafe extern "C" fn(Handle, u32) -> Status,
    prepare_network: unsafe extern "C" fn(Handle) -> Status,
    set_input: unsafe extern "C" fn(Handle, u32, Handle) -> Status,
    set_output: unsafe extern "C" fn(Handle, u32, Handle) -> Status,
    run_network: unsafe extern "C" fn(Handle) -> Status,
    finish_network: unsafe extern "C" fn(Handle) -> Status,
    destroy_network: unsafe extern "C" fn(Handle) -> Status,
    // Last, so it is closed after every pointer above is done with.
    _library: Library,
}

impl Api {
    fn open(path: &Path) -> Result<Self, String> {
        // SAFETY: the vendor's library, whose initialisers only register
        // the driver's user space; it is kept open as long as `Api`.
        let library = unsafe { Library::new(path) }
            .map_err(|error| format!("{} can't be opened: {error}", path.display()))?;
        macro_rules! get {
            ($name:literal) => {
                // SAFETY: each type is the function's signature in
                // vip_lite.h; the pointer is used only while `library` is
                // open (it is a field of the same `Api`, dropped last).
                *unsafe { library.get($name) }
                    .map_err(|error| format!("{}: no {}: {error}", path.display(), $name))?
            };
        }
        Ok(Self {
            init: get!("vip_init"),
            destroy: get!("vip_destroy"),
            create_network: get!("vip_create_network"),
            query_network: get!("vip_query_network"),
            query_input: get!("vip_query_input"),
            query_output: get!("vip_query_output"),
            create_buffer: get!("vip_create_buffer"),
            map_buffer: get!("vip_map_buffer"),
            unmap_buffer: get!("vip_unmap_buffer"),
            destroy_buffer: get!("vip_destroy_buffer"),
            flush_buffer: get!("vip_flush_buffer"),
            prepare_network: get!("vip_prepare_network"),
            set_input: get!("vip_set_input"),
            set_output: get!("vip_set_output"),
            run_network: get!("vip_run_network"),
            finish_network: get!("vip_finish_network"),
            destroy_network: get!("vip_destroy_network"),
            _library: library,
        })
    }
}

fn ok(status: Status, what: &str) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!("vip {what} failed: status {status}"))
    }
}

/// One mapped buffer: an input or an output.
struct Buffer {
    handle: Handle,
    ptr: *mut u8,
    bytes: usize,
    /// Outermost first.
    dims: Vec<usize>,
    format: Format,
    quant: Quant,
}

/// A loaded `.nb`.
pub struct VipNet {
    api: Arc<Api>,
    network: Handle,
    input: Buffer,
    outputs: Vec<Buffer>,
    shape: Input,
    /// Whether `vip_prepare_network` ran, so `vip_finish_network` is owed.
    prepared: bool,
}

// SAFETY: the handles are used by one thread at a time: the service holds
// its one net behind a lock, and VIPLite's handles are not tied to the
// thread that made them.
#[allow(unsafe_code)]
unsafe impl Send for VipNet {}

impl VipNet {
    fn buffer(api: &Api, network: Handle, index: u32, input: bool) -> Result<Buffer, String> {
        let query = if input {
            api.query_input
        } else {
            api.query_output
        };
        let mut dims_n: u32 = 0;
        let mut sizes = [0u32; MOST_DIMS];
        let mut data_format: i32 = 0;
        let mut quant_format: i32 = 0;
        // SAFETY: each property writes one value of the type given
        // (PROP_SIZES up to MOST_DIMS u32s) into memory this frame owns.
        unsafe {
            ok(
                query(network, index, PROP_NUM_DIMS, (&raw mut dims_n).cast()),
                "query dims",
            )?;
            ok(
                query(network, index, PROP_SIZES, sizes.as_mut_ptr().cast()),
                "query sizes",
            )?;
            ok(
                query(
                    network,
                    index,
                    PROP_DATA_FORMAT,
                    (&raw mut data_format).cast(),
                ),
                "query format",
            )?;
            ok(
                query(
                    network,
                    index,
                    PROP_QUANT_FORMAT,
                    (&raw mut quant_format).cast(),
                ),
                "query quantisation",
            )?;
        }
        let n = dims_n as usize;
        if n == 0 || n > MOST_DIMS {
            return Err(format!("a buffer of {n} dimensions"));
        }
        let format = Format::from_vip(data_format).ok_or(format!("buffer format {data_format}"))?;
        known_quant(quant_format)?;
        let mut quant_data = QuantData {
            affine: Affine {
                scale: 1.0,
                zero_point: 0,
            },
        };
        let quant = match quant_format {
            QUANT_DFP => {
                let mut pos: i32 = 0;
                // SAFETY: one i32, as above.
                ok(
                    unsafe { query(network, index, PROP_FIXED_POINT_POS, (&raw mut pos).cast()) },
                    "query fixed point",
                )?;
                quant_data.fixed_point_pos = pos;
                Quant::Dfp { pos }
            }
            QUANT_AFFINE => {
                let (mut scale, mut zero): (f32, i32) = (0.0, 0);
                // SAFETY: one f32 and one i32, as above.
                unsafe {
                    ok(
                        query(network, index, PROP_SCALE, (&raw mut scale).cast()),
                        "query scale",
                    )?;
                    ok(
                        query(network, index, PROP_ZERO_POINT, (&raw mut zero).cast()),
                        "query zero point",
                    )?;
                }
                quant_data.affine = Affine {
                    scale,
                    zero_point: zero,
                };
                Quant::Affine { scale, zero }
            }
            _ => Quant::None,
        };
        let params = BufferParams {
            num_of_dims: dims_n,
            sizes,
            data_format,
            quant_format,
            quant_data,
            memory_type: 0,
        };
        let mut handle: Handle = std::ptr::null_mut();
        // SAFETY: `params` is the struct vip_lite.h declares, its size given.
        ok(
            unsafe {
                (api.create_buffer)(
                    &raw const params,
                    std::mem::size_of::<BufferParams>() as u32,
                    &raw mut handle,
                )
            },
            "create buffer",
        )?;
        // Innermost first in VIPLite, outermost first here.
        let dims: Vec<usize> = sizes[..n].iter().rev().map(|&d| d as usize).collect();
        let bytes = dims.iter().product::<usize>() * format.bytes();
        // SAFETY: a buffer just made; mapped until `Drop`.
        let ptr = unsafe { (api.map_buffer)(handle) }.cast::<u8>();
        if ptr.is_null() {
            // SAFETY: the buffer just made, not mapped.
            unsafe { (api.destroy_buffer)(handle) };
            return Err("vip map buffer gave nothing".to_owned());
        }
        Ok(Buffer {
            handle,
            ptr,
            bytes,
            dims,
            format,
            quant,
        })
    }

    fn open(api: Arc<Api>, model: &Path) -> Result<Self, String> {
        let path = CString::new(model.as_os_str().as_encoded_bytes())
            .map_err(|_| "the model's path holds a NUL".to_owned())?;
        let mut network: Handle = std::ptr::null_mut();
        // SAFETY: a NUL-terminated path, read by the library while it loads.
        let made = ok(
            unsafe {
                (api.create_network)(path.as_ptr().cast(), 0, CREATE_FROM_FILE, &raw mut network)
            },
            "create network",
        );
        if let Err(why) = made {
            // SAFETY: undoes the caller's `vip_init`; no net holds it.
            unsafe { (api.destroy)() };
            return Err(why);
        }
        let mut net = Self {
            api: api.clone(),
            network,
            input: Buffer {
                handle: std::ptr::null_mut(),
                ptr: std::ptr::null_mut(),
                bytes: 0,
                dims: Vec::new(),
                format: Format::U8,
                quant: Quant::None,
            },
            outputs: Vec::new(),
            shape: Input {
                width: 0,
                height: 0,
                planar: false,
            },
            prepared: false,
        };
        let (mut inputs, mut outputs): (u32, u32) = (0, 0);
        // SAFETY: one u32 each.
        unsafe {
            ok(
                (api.query_network)(network, NETWORK_INPUTS, (&raw mut inputs).cast()),
                "query inputs",
            )?;
            ok(
                (api.query_network)(network, NETWORK_OUTPUTS, (&raw mut outputs).cast()),
                "query outputs",
            )?;
        }
        if inputs != 1 {
            return Err(format!("the model has {inputs} inputs: one frame is taken"));
        }
        if outputs == 0 || outputs > MOST_OUTPUTS {
            return Err(format!("the model has {outputs} outputs"));
        }
        net.input = Self::buffer(&api, network, 0, true)?;
        for index in 0..outputs {
            let buffer = Self::buffer(&api, network, index, false)?;
            net.outputs.push(buffer);
        }
        net.shape = frame_shape(&net.input.dims, net.input.format)?;
        // SAFETY: the network and the buffers made above.
        unsafe {
            ok((api.prepare_network)(network), "prepare")?;
            net.prepared = true;
            ok((api.set_input)(network, 0, net.input.handle), "set input")?;
            for (index, buffer) in net.outputs.iter().enumerate() {
                ok(
                    (api.set_output)(network, index as u32, buffer.handle),
                    "set output",
                )?;
            }
        }
        Ok(net)
    }
}

/// A buffer's quantisation this build reads: none, dynamic fixed point or
/// affine per tensor. Per-channel affine (3) or anything newer, read as
/// plain numbers, would be wrong without a word, so its model is refused
/// (Heron's P3 on #7972).
fn known_quant(code: i32) -> Result<(), String> {
    match code {
        QUANT_NONE | QUANT_DFP | QUANT_AFFINE => Ok(()),
        other => Err(format!(
            "buffer quantisation {other} is not one this lamp reads (none, dynamic fixed point or affine per tensor)"
        )),
    }
}

/// The frame a model's input takes, from its shape (outermost first): one
/// RGB frame of bytes, `n, h, w, 3` or `n, 3, h, w`.
///
/// The converter's metadata tags MoveNet's input `nchw` while its shape is
/// `[1, 192, 192, 3]`: the tag is a label only and not trusted. The shape
/// is what is read, so that one is interleaved (HWC), which a planar run
/// against float MoveNet confirmed on lamp-0c89 (Heron's P2 2 on #7972).
pub fn frame_shape(dims: &[usize], format: Format) -> Result<Input, String> {
    if format != Format::U8 {
        return Err(format!(
            "the model's input is {format:?}: a frame of unsigned bytes is taken (convert with the normalisation baked in)"
        ));
    }
    match dims {
        [1, height, width, 3] => Ok(Input {
            width: *width,
            height: *height,
            planar: false,
        }),
        [1, 3, height, width] => Ok(Input {
            width: *width,
            height: *height,
            planar: true,
        }),
        other => Err(format!(
            "the model's input is {other:?}: one RGB frame is taken"
        )),
    }
}

impl Net for VipNet {
    fn input(&self) -> Input {
        self.shape
    }

    fn run(&mut self, rgb: &[u8]) -> Result<(Vec<Tensor>, RunMs), String> {
        if rgb.len() != self.input.bytes {
            return Err(format!(
                "the frame is {} bytes, the model takes {}",
                rgb.len(),
                self.input.bytes
            ));
        }
        let started = Instant::now();
        // SAFETY: the input buffer is mapped and `bytes` long (checked above).
        unsafe {
            std::ptr::copy_nonoverlapping(rgb.as_ptr(), self.input.ptr, rgb.len());
            ok((self.api.flush_buffer)(self.input.handle, FLUSH), "flush")?;
        }
        let copied = Instant::now();
        // SAFETY: a prepared network with its buffers set.
        ok(unsafe { (self.api.run_network)(self.network) }, "run")?;
        let ran = Instant::now();
        let mut tensors = Vec::with_capacity(self.outputs.len());
        for buffer in &self.outputs {
            // SAFETY: a mapped output buffer, `bytes` long; invalidated so
            // the CPU reads what the NPU wrote.
            let data = unsafe {
                ok(
                    (self.api.flush_buffer)(buffer.handle, INVALIDATE),
                    "invalidate",
                )?;
                std::slice::from_raw_parts(buffer.ptr, buffer.bytes).to_vec()
            };
            tensors.push(Tensor {
                dims: buffer.dims.clone(),
                format: buffer.format,
                quant: buffer.quant,
                data,
            });
        }
        let ms = |from: Instant, to: Instant| (to - from).as_secs_f64() * 1e3;
        Ok((
            tensors,
            RunMs {
                input: ms(started, copied),
                npu: ms(copied, ran),
            },
        ))
    }
}

impl Drop for VipNet {
    fn drop(&mut self) {
        // SAFETY: the handles this net made, each let go once, in the
        // reference binding's order; the library itself stays open until
        // the last `Arc<Api>` goes.
        unsafe {
            if self.prepared {
                (self.api.finish_network)(self.network);
            }
            for buffer in std::iter::once(&self.input).chain(&self.outputs) {
                if !buffer.handle.is_null() {
                    (self.api.unmap_buffer)(buffer.handle);
                    (self.api.destroy_buffer)(buffer.handle);
                }
            }
            if !self.network.is_null() {
                (self.api.destroy_network)(self.network);
            }
            (self.api.destroy)();
        }
    }
}

/// The real backend: VIPLite from `library`, with the NPU at `device`.
pub struct Vip {
    pub library: PathBuf,
    pub device: PathBuf,
}

impl Default for Vip {
    fn default() -> Self {
        Self {
            library: LIBRARY.into(),
            device: DEVICE.into(),
        }
    }
}

impl Backend for Vip {
    fn load(&self, model: &Path) -> Result<Box<dyn Net>, LoadError> {
        if !self.device.exists() {
            return Err(LoadError::Unsupported(format!(
                "this lamp has no NPU ({} is not there)",
                self.device.display()
            )));
        }
        let api = Arc::new(Api::open(&self.library).map_err(LoadError::Unsupported)?);
        // SAFETY: no arguments; undone by `vip_destroy` in the net's `Drop`,
        // or below when the net is not made.
        ok(unsafe { (api.init)() }, "init").map_err(LoadError::Unsupported)?;
        match VipNet::open(api, model) {
            Ok(net) => Ok(Box::new(net)),
            // A net half made was dropped inside `open`, which destroyed
            // what it had made and called `vip_destroy`.
            Err(why) => Err(LoadError::Failed(why)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buffer_parameters_are_vip_lites_layout() {
        // 4 + 6*4 + 4 + 4 + 8 + 4, as the reference binding's ctypes struct.
        assert_eq!(std::mem::size_of::<BufferParams>(), 48);
    }

    #[test]
    fn a_frame_is_one_rgb_picture_of_bytes() {
        assert_eq!(
            frame_shape(&[1, 480, 640, 3], Format::U8),
            Ok(Input {
                width: 640,
                height: 480,
                planar: false
            })
        );
        assert_eq!(
            frame_shape(&[1, 3, 192, 192], Format::U8).map(|input| input.planar),
            Ok(true)
        );
        assert!(frame_shape(&[1, 192, 192, 3], Format::F32).is_err());
        assert!(frame_shape(&[1, 192, 192, 1], Format::U8).is_err());
    }

    #[test]
    fn an_unknown_quantisation_refuses_the_model() {
        for known in [QUANT_NONE, QUANT_DFP, QUANT_AFFINE] {
            assert_eq!(known_quant(known), Ok(()));
        }
        assert!(known_quant(3).unwrap_err().contains("quantisation 3"));
        assert!(known_quant(-1).is_err());
    }

    #[test]
    fn a_lamp_without_the_npu_is_unsupported() {
        let vip = Vip {
            library: "/nonexistent/libNBGlinker.so".into(),
            device: "/nonexistent/vipcore".into(),
        };
        assert!(matches!(
            vip.load(Path::new("/nonexistent/model.nb")),
            Err(LoadError::Unsupported(_))
        ));
        let vip = Vip {
            library: "/nonexistent/libNBGlinker.so".into(),
            device: std::env::temp_dir(),
        };
        assert!(matches!(
            vip.load(Path::new("/nonexistent/model.nb")),
            Err(LoadError::Unsupported(_))
        ));
    }
}
