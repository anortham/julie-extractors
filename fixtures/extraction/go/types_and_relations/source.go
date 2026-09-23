// Package shapes exercises Go type relations, docs, and inferred bindings.
package shapes

import (
	"errors"
	"io"
	"sync"
)

// Closer releases a resource.
type Closer interface{ Close() error }

// ReadCloser embeds a qualified interface and a local one.
type ReadCloser interface {
	io.Reader
	Closer
}

// Base carries the shared name.
type Base struct{ Name string }

// Circle embeds Base and a qualified mutex.
type Circle struct {
	Base
	*sync.Mutex
	Radius float64
}

// Close satisfies Closer.
func (c *Circle) Close() error { return nil }

var _ Closer = (*Circle)(nil)

// ID is an exported alias.
type ID = string

type label = string

// Pair is a generic definition.
type Pair[K comparable, V any] struct {
	Key   K
	Value V
}

// Limit caps a batch.
//
//go:generate stringer -type=Limit
const Limit = 10

// DefaultCircle is the zero circle.
var DefaultCircle = &Circle{}

// NewCircle builds a circle.
func NewCircle(r float64) (*Circle, error) {
	if r < 0 {
		return nil, errors.New("negative")
	}
	return &Circle{Radius: r}, nil
}

// Identity returns its argument.
func Identity[T any](value T) T { return value }

func Shapes(items []Circle, index map[string]*Circle, events chan Base, value any) int {
	built, err := NewCircle(1)
	if err != nil {
		return 0
	}
	fresh := new(Base)
	_ = fresh
	_ = built
	total := len(items)
	names := make([]string, 0, total)
	_ = names
	for position, item := range items {
		_ = position
		_ = item.Radius
	}
	for key, circle := range index {
		_ = key
		_ = circle.Radius
	}
	for event := range events {
		_ = event.Name
	}
	switch typed := value.(type) {
	case *Circle:
		_ = typed.Radius
	case Base:
		_ = typed.Name
	}
	_ = Identity[int](3)
	_ = float64(total)
	_ = label("x")
	return total
}
