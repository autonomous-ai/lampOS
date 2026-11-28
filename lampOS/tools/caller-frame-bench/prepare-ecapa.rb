# frozen_string_literal: true

# Repack the pinned Apache-2.0 SpeechBrain embedding checkpoint for tract.
# The JSON describes the ONNX operators, tensor dimensions and byte offsets
# in this exact upstream ZIP. There are no learned values in the recipe.
require 'digest'
require 'json'

SOURCE = '0575cb64845e6b9a10db9bcb74d5ac32b326b8dc90352671d345e2ee3d0126a2'
GRAPH = 'd8c32b0dbc17e31c7b13c9d7910f0adaca2ea0b0b3351da05041c9168e741994'

# Standard ONNX protobuf wire types. This narrow writer keeps preparation
# usable with stock Ruby on a machine without Python, protoc or a model SDK.
def varint(value)
  bytes = []
  while value >= 128
    bytes << (128 | (value & 127))
    value >>= 7
  end
  (bytes << value).pack('C*')
end

def integer(field, value)
  varint(field << 3) + varint(value)
end

def bytes(field, value)
  varint((field << 3) | 2) + varint(value.bytesize) + value.b
end

def attribute(name, value)
  type, payload = case value
                  when Float then [1, varint((2 << 3) | 5) + [value].pack('e')]
                  when Integer then [2, integer(3, value)]
                  when String then [3, bytes(4, value)]
                  when Array then [7, value.map { |n| integer(8, n) }.join]
                  else raise "invalid ONNX attribute #{name}"
                  end
  bytes(1, name) + payload + integer(20, type)
end

def node(op, name, inputs, outputs, attrs)
  out = inputs.map { |x| bytes(1, x) }.join + outputs.map { |x| bytes(2, x) }.join
  out << bytes(3, name) unless name.empty?
  out << bytes(4, op)
  attrs.each { |key, value| out << bytes(5, attribute(key, value)) }
  out
end

def tensor(source, name, type, dims, location)
  payload = if location.key?('values')
              location.fetch('values').pack(type == 1 ? 'e*' : 'q<*')
            elsif location.key?('stride')
              # The three attention-convolution inputs share an upstream
              # [128, 9216, 1] tensor. Each uses 3072 channels of every row.
              (0...location.fetch('rows')).map do |row|
                source.pread(location.fetch('width'), location.fetch('offset') + row * location.fetch('stride'))
              end.join
            else
              source.pread(dims.reduce(1, :*) * 4, location.fetch('offset'))
            end
  dims.map { |n| integer(1, n) }.join + integer(2, type) + bytes(8, name) + bytes(9, payload)
end

def value_info(name, dimensions)
  shape = dimensions.map do |d|
    bytes(1, d.is_a?(Integer) ? integer(1, d) : bytes(2, d))
  end.join
  bytes(1, name) + bytes(2, bytes(1, integer(1, 1) + bytes(2, shape)))
end

def convert(source, recipe)
  nodes = recipe.fetch('nodes')
  graph = nodes.map { |n| bytes(1, node(*n)) }.join
  graph << bytes(2, 'SpeechBrain ECAPA-TDNN VoxCeleb')
  recipe.fetch('tensors').each { |t| graph << bytes(5, tensor(source, *t)) }
  graph << bytes(11, value_info('fbank', [1, 80, 'T']))
  graph << bytes(12, value_info(nodes.last.fetch(3).fetch(0), [1, 192, 1]))
  integer(1, 8) + bytes(2, 'lampOS') + bytes(7, graph) + bytes(8, bytes(1, '') + integer(2, 11))
end

if $PROGRAM_NAME == __FILE__
  abort 'usage: ruby prepare-ecapa.rb OFFICIAL-embedding_model.ckpt NEW-ecapa-voxceleb.onnx' unless ARGV.length == 2
  original, destination = ARGV
  abort 'unexpected SpeechBrain checkpoint SHA-256' unless Digest::SHA256.file(original).hexdigest == SOURCE
  recipe = JSON.parse(File.read(File.join(__dir__, 'ecapa-voxceleb-graph.json')))
  graph = File.open(original, 'rb') { |f| convert(f, recipe) }
  abort 'exported graph differs from the verified artifact' unless Digest::SHA256.hexdigest(graph) == GRAPH
  File.open(destination, 'wbx', 0o600) { |f| f.write(graph) }
  puts "#{GRAPH}  #{destination}"
end
