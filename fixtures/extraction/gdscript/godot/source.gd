@tool
@icon("res://icons/player.svg")
class_name Player
extends "res://actors/actor.gd"

signal died
signal _health_changed(value: int)

enum _Mode { IDLE, RUN }

const Bullet = preload("res://player/bullet.tscn")
const _MAX_SPEED := 400.0

@export var speed: float = 200.0
@export_range(0, 100) var max_health: int = 100
@export_group("Visuals")
@export var tint: Color
var state: int = 0
var _cache := {}
@onready var sprite: AnimatedSprite2D = $AnimatedSprite2D
@onready var camera := %Camera as Camera2D
@onready var hud = get_node("UI/Hud")

@warning_ignore("unused_private_class_variable")
var _debug_label: Label


func _ready() -> void:
	$Hurtbox.area_entered.connect(_on_hurtbox_area_entered)
	died.connect(_on_died.bind(1))
	connect("died", _on_died)
	reset()


func reset() -> void:
	var level = load("res://levels/level_1.tscn") as PackedScene
	var config = ResourceLoader.load("res://data/config.tres")
	state = level.get_state().get_node_count() + config.size()


func lookup(key: StringName) -> Enemy.Kind:
	var inner: Outer.Inner = Outer.Inner.new()
	return inner.kind_for(key)


func _on_hurtbox_area_entered(area: Area2D) -> void:
	if area is Projectile:
		(area as Projectile).explode()
	died.emit()
	emit_signal("died")


func _on_died(_code: int = 0) -> void:
	pass


@rpc("any_peer", "call_local", "reliable")
func sync_speed(value: float) -> void:
	speed = value


@abstract func describe() -> String


class _Inventory:
	func clear() -> void:
		reset()

	func reset() -> void:
		pass
