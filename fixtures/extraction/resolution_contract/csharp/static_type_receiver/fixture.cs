// Static-type receiver for C#: the receiver names a type directly rather than a
// variable whose type must be inferred, so no `type_facts` row participates.
// `Fixture.Create()` and `Color.Red` both resolve at tier3_static_type (0.70)
// from another file, because `Fixture` and `Color` are public and top-level.
namespace App
{
    public class Fixture
    {
        public static int Create() { return 1; }
    }

    public enum Color
    {
        Red,
        Blue
    }

    public class Limits
    {
        public const int Max = 10;
    }
}
