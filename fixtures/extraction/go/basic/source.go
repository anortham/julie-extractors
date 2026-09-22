package fixture

import (
	"net/http"

	"gopkg.in/yaml.v3"
)

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

func (w Worker) Start() {
	next := NewWorker(w.ID)
	_ = next
	w.Run()
	other := Worker{}
	other.Run()
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

type (
	// Store persists workers.
	Store interface {
		// Load reads one worker.
		Load(id int) (Worker, error)
	}
	// memStore keeps workers in memory.
	memStore struct {
		items map[int]Worker
	}
)

// Decode parses a worker document.
func Decode(data []byte) (w Worker, err error) {
	err = yaml.Unmarshal(data, &w)
	return
}

func (s memStore) Load(id int) (Worker, error) {
	return s.items[id], nil
}

func (w Worker) Restart() {
	NewWorker(w.ID).Run()
}
