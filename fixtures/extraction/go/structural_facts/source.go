package main

func worker() {}
func cleanup() {}

func run() {
	go worker()
	defer cleanup()
}
