// Overload-stays-ambiguous for C#: two same-name methods (`Handle`) are
// kind-compatible candidates. Without arity/signature discrimination (follow-on
// F3) the reference has more than one candidate, so it must stay unresolved —
// no best guess, target stays NULL.
namespace App
{
    public static class Api
    {
        public static int Handle(int x) { return x; }
        public static int Handle(string s) { return 0; }
    }

    public class Client
    {
        public int Run()
        {
            return Api.Handle(1);
        }
    }
}
