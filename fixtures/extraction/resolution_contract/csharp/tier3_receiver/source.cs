// Tier 3 (receiver-typed) for C#: the receiver `widget` is a field typed
// `Widget`; a `type_facts` row records that type. The call `widget.Build()`
// resolves through receiver -> type_fact -> unique type symbol -> member with
// the terminal name (tier3_receiver). Confidence is CONFIDENCE_TIER3_INFERRED
// (0.65) because the field's type fact is inferred.
namespace App
{
    public class Widget
    {
        public int Build() { return 1; }
    }

    public class Consumer
    {
        private Widget widget;

        public int Run()
        {
            return widget.Build();
        }
    }
}
