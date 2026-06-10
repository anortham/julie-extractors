package fixture

type Worker struct {
	ID int
}

func NewWorker(id int) Worker {
	return Worker{ID: id}
}

func (w Worker) Run() int {
	return helper(w.ID)
}

// helper increments a worker id.
func helper(value int) int {
	return value + 1
}
