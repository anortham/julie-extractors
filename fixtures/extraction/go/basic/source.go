package fixture

import "net/http"

type List[T any] struct{}

type Map[K, V any] struct{}

type Worker struct {
	ID int `json:"id" db:"worker_id"`
}

var workerIndex Map[string, List[int]]

func NewWorker(id int) Worker {
	return Worker{ID: id}
}

func (w Worker) Run() int {
	recordRun(w.ID)
	return helper(w.ID)
}

// recordRun emits a worker-run marker for observability hooks.
func recordRun(id int) {
	observeRun("worker-run", id)
}

// observeRun records a named worker event for downstream hooks.
func observeRun(event string, id int) {
	_ = event
	_ = id
}

// helper increments a worker id.
func helper(value int) int {
	return value + 1
}

// FetchStatus checks the worker service health endpoint.
func FetchStatus() error {
	_, err := http.Get("https://api.example.com/workers/status")
	return err
}

//go:noinline
func Evaluate(count int, enabled bool) int {
	total := 0
	if enabled {
		for i := 0; i < count; i++ {
			total += i
		}
	}
	return total
}
