module example.com/negative

go 1.21

require example.com/direct v1.0.0

replace example.com/direct => example.com/fork v1.0.1

exclude example.com/direct v0.9.0
