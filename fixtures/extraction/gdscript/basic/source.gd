class_name Worker
extends Node

signal activated(value)

@export var id: int

func _init(value: int) -> void:
    id = value

func run() -> int:
    record_run(id)
    return helper(id)

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
