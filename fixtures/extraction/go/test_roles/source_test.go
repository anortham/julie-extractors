// Package roles exercises the Go test-role contract across the standard
// testing package, a testify suite, a gocheck suite, and Ginkgo.
package roles

import (
	"sync"
	"testing"

	. "github.com/onsi/ginkgo/v2"
	"github.com/stretchr/testify/suite"
)

func TestMain(m *testing.M) {
	m.Run()
}

func TestAdds(t *testing.T) {}

func BenchmarkAdds(b *testing.B) {}

func FuzzAdds(f *testing.F) {}

func ExampleAdds() {}

func Testable(t *testing.T) {}

func AddsLikeATest(t *testing.T) {}

func helperAdds(left int, right int) int {
	return left + right
}

type CalculatorSuite struct {
	suite.Suite
	total int
}

func (s *CalculatorSuite) SetupSuite() {}

func (s *CalculatorSuite) SetupTest() {}

func (s *CalculatorSuite) BeforeTest(suiteName string, testName string) {}

func (s *CalculatorSuite) TestAddition() {}

func (s *CalculatorSuite) AfterTest(suiteName string, testName string) {}

func (s *CalculatorSuite) TearDownTest() {}

func (s *CalculatorSuite) TearDownSuite() {}

func (s *CalculatorSuite) reset() {}

type GoCheckSuite struct {
	value int
}

func (s *GoCheckSuite) SetUpSuite() {}

func (s *GoCheckSuite) SetUpTest() {}

func (s *GoCheckSuite) TestDivides() {}

func (s *GoCheckSuite) TearDownSuite() {}

type recordingClock struct {
	sync.Mutex
	now int64
}

func (c *recordingClock) Advance(delta int64) {}

var _ = Describe("calculator", func() {
	Context("addition", func() {
		BeforeEach(func() {})
		AfterEach(func() {})
		It("adds two numbers", func() {})
	})
})

func orphanGinkgoHelper() {
	It("never runs without a container", func() {})
}
