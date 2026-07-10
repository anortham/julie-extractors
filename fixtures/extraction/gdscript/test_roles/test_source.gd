extends GutTest

func before_each():
    pass

func test_addition():
    assert_eq(2 + 2, 4)

func verify_addition():
    return 2 + 2 == 4
