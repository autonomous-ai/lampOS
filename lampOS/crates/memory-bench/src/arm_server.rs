//! Isolated JSON-lines reference arm process. It never opens evaluator files.
use crate::{
    Error, Result,
    arms::{Basic, BasicKind, MemoryArm},
    invalid,
    partial::Partial,
    protocol::*,
    session::Session,
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::Path,
};

pub fn serve<R: BufRead, W: Write>(reader: &mut R, writer: &mut W, revision: &str) -> Result<()> {
    if revision.len() != 40 || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("arm requires full source revision"));
    }
    let mut session = Session::default();
    let mut arm: Option<Box<dyn MemoryArm>> = None;
    while let Some(value) = read_frame(reader)? {
        let request: Request = serde_json::from_value(value)?;
        let result = (|| -> Result<(Session, Value)> {
            let next = session.prepare(&request)?;
            let data = if request.op == Op::Init {
                let init: Init = request.decode()?;
                let path = Path::new(&init.state_dir);
                let metadata = std::fs::symlink_metadata(path)?;
                if !metadata.is_dir()
                    || metadata.file_type().is_symlink()
                    || std::fs::read_dir(path)?.next().is_some()
                {
                    return Err(invalid("arm requires a fresh real state directory"));
                }
                if init.config.as_object().is_none_or(|c| !c.is_empty()) {
                    return Err(invalid("reference arm has no configuration switches"));
                }
                arm = Some(match init.arm.as_str() {
                    "none" => Box::new(Basic::open(BasicKind::None, init.owner_id.clone(), path)?),
                    "full_history" => Box::new(Basic::open(
                        BasicKind::FullHistory,
                        init.owner_id.clone(),
                        path,
                    )?),
                    "lampos_today_partial" => Box::new(Partial::open(&init)?),
                    _ => {
                        return Err(Error::Unsupported(
                            "unknown reference arm; complete current-product baseline unavailable"
                                .into(),
                        ));
                    }
                });
                json!({"arm":init.arm,"implementation_revision":revision,"capabilities":["ingest","sleep","forget","probe_begin","snapshot","recall","tool","inspect","probe_end","close"]})
            } else {
                let arm = arm.as_mut().ok_or_else(|| invalid("arm not initialized"))?;
                match request.op {
                    Op::Init => unreachable!(),
                    Op::Ingest => arm.ingest(request.decode::<Ingest>()?.event)?,
                    Op::Sleep => arm.sleep(request.decode()?)?,
                    Op::Forget => arm.forget(request.decode()?)?,
                    Op::Snapshot => {
                        serde_json::to_value(arm.snapshot(request.decode::<QueryData>()?.query)?)?
                    }
                    Op::Recall => {
                        serde_json::to_value(arm.recall(request.decode::<QueryData>()?.query)?)?
                    }
                    Op::Tool => arm.tool(request.decode()?)?,
                    Op::Inspect => arm.inspect(&request.decode::<Inspect>()?.person_id)?,
                    Op::ProbeBegin => {
                        let input: ProbeBegin = request.decode()?;
                        arm.begin_probe(&input.probe_id, input.at_unix_ms)?;
                        json!({"isolated":true})
                    }
                    Op::ProbeEnd => {
                        arm.end_probe(&request.decode::<ProbeEnd>()?.probe_id)?;
                        json!({"restored":true})
                    }
                    Op::Close => json!({}),
                }
            };
            Ok((next, data))
        })();
        let response = match result {
            Ok((next, data)) => {
                session = next;
                Response::success(&request.id, data)
            }
            Err(error) => {
                let code = if matches!(error, Error::Unsupported(_)) {
                    "unsupported"
                } else {
                    "arm_failed"
                };
                let mut reply = Response::failure(&request.id, code, &error.to_string());
                reply.v = request.v;
                write_frame(writer, &reply)?;
                return Err(error);
            }
        };
        let mut response = response;
        response.v = request.v;
        write_frame(writer, &response)?;
        if request.op == Op::Close {
            return Ok(());
        }
    }
    Err(invalid("arm input ended before close"))
}
