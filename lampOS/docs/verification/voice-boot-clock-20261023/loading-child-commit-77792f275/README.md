# Loading-child commit binding

The focused receipt was collected before the commit. Its exact source bytes
were committed as `77792f2753b4fb97f4a078897cdf52e1bbd24362`, on
`62b1120fc1af5462529f0c76089a322500e30929`. The complete source aggregate
and both changed file hashes are in [the binding](commit-binding.json).
Original observation timestamps and uncommitted-source labels are retained.

The old clock patch first carried unchanged as `8ae865187`; that source is
pushed on `tests/1569-boot-clock-train34-control` for the control. The
[range-diff](range-diff.json) then records the loading-child P3 added to
#3190. Full qualification of the new head is pending. The reader's earlier
execution HOLD and every failed gate remain in force.

The public range-diff omits its author metadata line. The [projection record](range-diff-projection.json) binds the original and published bytes; patch lines are unchanged.

The range-diff is stored as a lossless UTF-8 JSON string so its patch context spaces survive the documentation whitespace check.
