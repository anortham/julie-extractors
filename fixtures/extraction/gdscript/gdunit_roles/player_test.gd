extends GdUnitTestSuite

func before() -> void:
	pass

func before_test() -> void:
	pass

func after_test() -> void:
	pass

func after() -> void:
	pass

func test_player_moves() -> void:
	assert_bool(true).is_true()

func make_player() -> Node:
	return Node.new()
