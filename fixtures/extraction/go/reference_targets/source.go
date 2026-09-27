package reference_targets

type Worker struct{}

func shared() {}

func (Worker) shared() {}

func (Worker) hidden() {}

func genericTarget[T any]() {}

func methodOnlyCaller() {
	hidden()
}

func packageCaller() {
	shared()
}

func genericSiblingCaller() {
	genericTarget[int]()
}

func genericShadow(genericTarget int) {
	genericTarget[int]()
}

func parameterShadow(shared int) {
	shared()
}

func localShadow() {
	shared := func() {}
	shared()
}

func receiverPending(worker Worker) {
	worker.shared()
}
