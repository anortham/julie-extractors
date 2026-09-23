package suites

import (
	"testing"

	g "github.com/onsi/ginkgo/v2"
	check "gopkg.in/check.v1"
)

func Test(t *testing.T) { check.TestingT(t) }

type StoreSuite struct {
	dir string
}

var _ = check.Suite(&StoreSuite{})

func (s *StoreSuite) SetUpTest(c *check.C) {}

func (s *StoreSuite) TestSaves(c *check.C) {}

type Registry struct{}

func (r *Registry) Suite(value any) any { return value }

type Unregistered struct{}

var registry = &Registry{}

var _ = registry.Suite(&Unregistered{})

var _ = g.Describe("store", func() {
	g.It("saves", func() {})
})
