class_name Inventory extends Enemy
## Holds item stacks for one character.
##
## Stacks merge when they share an item id.

## Emitted when a stack changes.
signal changed(slot: int)

## Largest stack size.
const LIMIT := 99

@onready var timer: Timer = get_node("Timer")
var pool := ObjectPool.new(16)

## Current health, clamped to the valid range.
var health: int = 100:
	set(value):
		health = clampi(value, 0, 100)
		refresh_ui()
	get:
		return compute()

var ratio: float:
	get = get_ratio, set = set_ratio

enum Flags {
	## The first flag.
	FLAG_A,
	FLAG_B = 2,
}

class ItemStack extends RefCounted:
	var count: int = 0

	func add(n: int) -> void:
		count += n

	class Nested:
		func deep() -> void:
			pass

	func push(x) -> void:
		add(x)

class Special extends ItemStack:
	func add(n: int) -> void:
		super.add(n * 2)

func _init(slots: int) -> void:
	setup(slots)
	Logger.info("inventory created")

func setup(slots: int) -> void:
	get_node("HUD").get_child(0).queue_free()
	var h = player.stats.health
	FileAccess.open(SAVE_PATH, FileAccess.WRITE)

func refresh_ui() -> void:
	pass

func compute() -> int:
	return 1

func get_ratio() -> float:
	return 1.0

func set_ratio(v: float) -> void:
	pass
