#!/usr/bin/env ruby
# Pack the original public tensors into the fixed layout read by the Rust child.
# The source checksums pin the names, order, shapes and little-endian dtype.
require "digest"
require "json"
require "open3"

abort "usage: ruby prepare-model.rb MODEL_DIRECTORY" unless ARGV.length == 1
dir = File.realpath(ARGV.fetch(0))
network = File.join(dir, "model.safetensors")
subspace = File.join(dir, "speaker_subspace.npz")
{
  network => "0f4d3a99b410eb410d534e57e82e68250437f2373f9f070e7ab3e987983022af",
  subspace => "61918aa46d7b6f88f682814e11d5099e71e2b5a8030454c1fded212ced1caedf",
}.each do |path, expected|
  abort "unexpected source SHA-256 for #{File.basename(path)}" unless Digest::SHA256.file(path).hexdigest == expected
end

bytes = File.binread(network)
header_size = bytes.unpack1("Q<")
header = JSON.parse(bytes.byteslice(8, header_size))
packed = String.new(encoding: Encoding::BINARY)
%w[feat_mean feat_std proj_lda proj_bias energy_threshold mel_mean_v1 mel_std_v1].each do |name|
  npy, err, status = Open3.capture3("unzip", "-p", subspace, "#{name}.npy", binmode: true)
  abort "read #{name}: #{err}" unless status.success?
  npy_header_end = 10 + npy.byteslice(8, 2).unpack1("v")
  packed << npy.byteslice(npy_header_end..-1)
end
10.times do |index|
  field = header.fetch(format("weight_%03d", index))
  first, last = field.fetch("data_offsets")
  abort "unexpected tensor dtype" unless field.fetch("dtype") == "F32"
  packed << bytes.byteslice(8 + header_size + first, last - first)
end
path = File.join(dir, "caller-frame.f32")
File.open(path, File::WRONLY | File::CREAT | File::EXCL, 0600) { |out| out.binmode.write(packed) }
puts "#{Digest::SHA256.hexdigest(packed)}  #{File.basename(path)}"
