class_name Worker
extends Node

signal activated(value)

@export var id: int
var worker_index: Array[Array[int]]

func _init(value: int) -> void:
    id = value

func run() -> int:
    record_run(id)
    return helper(id)

## Increment a worker id.
func helper(value: int) -> int:
    return value + 1

func record_run(worker_id: int) -> void:
    observe_run("worker-run", worker_id)

func observe_run(_event: String, _worker_id: int) -> void:
    pass

func fetch_status() -> void:
    fetch_url("https://api.example.com/workers/status")

func fetch_url(_url: String) -> void:
    pass

func evaluate(count: int, enabled: bool) -> int:
    var total = 0
    if enabled:
        for i in range(1, count + 1):
            total += i
    elif count > 0:
        total = 1
    match count % 3:
        0:
            total += 0
        1:
            total += 1
        _:
            total += 2
    return total

func typed_params(x: Foo, y := 2, z) -> void:
    var typed_local: Foo = null
    var inferred_local := Foo.new()
    var unknown_local = Unknown.new()
    var loaded = load("res://x.tscn").instantiate()
    var made = make()
    var items: Array[Foo]
    self.persist()
    super.restore()

class Foo:
    pass

class Bar extends Resource:
    func inner_run() -> void:
        self.persist()
        super.restore()
