"""Makes a tf2onnx conversion byte for byte the same from run to run.

tf2onnx names the constants it folds from a counter whose order changes
between runs (`const_fold_opt__1869`, `new_shape__1315`), and names the
batch dimension `unk__<n>`, and it writes the source's path in the
graph's doc string. Nothing else differs. This renames every
initializer by its first use, orders them so, calls the batch dimension
`batch`, empties the doc string, and writes the file deterministically.
The graph and the weights are untouched.

    python canon.py IN.onnx OUT.onnx
"""

import sys

import onnx


def main(source: str, target: str) -> None:
    model = onnx.load(source)
    graph = model.graph
    initializers = {i.name for i in graph.initializer}
    names: dict[str, str] = {}
    for node in graph.node:
        for k, name in enumerate(node.input):
            if name in initializers:
                names.setdefault(name, "const_%04d" % len(names))
                node.input[k] = names[name]
    for initializer in graph.initializer:
        initializer.name = names.get(initializer.name, initializer.name)
    first_use = {}
    for node in graph.node:
        for name in node.input:
            first_use.setdefault(name, len(first_use))
    ordered = sorted(graph.initializer, key=lambda i: (first_use.get(i.name, 1 << 30), i.name))
    del graph.initializer[:]
    graph.initializer.extend(ordered)
    for value in list(graph.input) + list(graph.output) + list(graph.value_info):
        for dim in value.type.tensor_type.shape.dim:
            if dim.dim_param:
                dim.dim_param = "batch"
    # tf2onnx writes the path it converted from here.
    graph.doc_string = ""
    onnx.checker.check_model(model)
    # Protobuf may order a message's bytes differently from run to run.
    with open(target, "wb") as out:
        out.write(model.SerializeToString(deterministic=True))


if __name__ == "__main__":
    main(*sys.argv[1:3])
