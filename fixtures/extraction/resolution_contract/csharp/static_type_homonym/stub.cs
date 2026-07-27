// A file-scoped helper that shares its simple name with a framework type. It is
// unreachable from any other file, so a `NullLoggerFactory.Instance` reference
// elsewhere in the workspace means the framework type, not this stub. The
// static-type receiver tier must refuse to bind it across files — otherwise
// type-name uniqueness reproduces the wrong-edge failure that tier 4's kind
// filter exists to prevent.
namespace App
{
    file static class NullLoggerFactory
    {
        public static int Instance = 1;
    }

    public class LocalUser
    {
        public int Read() { return NullLoggerFactory.Instance; }
    }
}
