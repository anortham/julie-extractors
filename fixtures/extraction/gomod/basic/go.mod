// Package app is the example service.
//
// Deprecated: use example.com/app/v2 instead.
module example.com/app

// The minimum Go version for this module.
go 1.22.0

toolchain go1.22.4

// Cobra drives the command line.
require github.com/spf13/cobra v1.8.0

require (
	// pflag parses the flags.
	github.com/spf13/pflag v1.0.5 // indirect
	"example.com/quoted" v0.1.0
	golang.org/x/text v0.14.0 // indirect; needed by golang.org/x/net
)

replace example.com/old v1.0.0 => example.com/new v1.1.0

replace (
	// Develop against the local checkout.
	example.com/lib => ../lib
	example.com/tools v0.2.0 => ./tools
	example.com/abs => /src/abs
)

exclude example.com/broken v0.9.0

exclude (
	example.com/bad v1.2.0
	example.com/bad v1.2.1
)

// Remote-triggered crash in the parser.
retract v1.0.5

// Releases cut from the wrong branch.
retract (
	v1.0.0 // Published accidentally.
	[v1.0.1, v1.0.4]
)

tool golang.org/x/tools/cmd/stringer

tool (
	example.com/app/cmd/gen
)

ignore ./node_modules

ignore (
	./third_party/js
	static
	x
)

// Keep the Go 1.20 behavior for these settings.
godebug (
	panicnil=1
	// Timers keep the old channel semantics.
	asynctimerchan=0
)

// Opt in to the Go 1.21 defaults.
godebug default=go1.21