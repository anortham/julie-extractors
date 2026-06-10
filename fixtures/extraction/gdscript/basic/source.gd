class_name Worker
extends Node

signal activated(value)

@export var id: int

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
