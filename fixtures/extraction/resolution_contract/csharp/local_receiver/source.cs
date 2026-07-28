// Local/param receivers: typed locals and parameters resolve member_access via tier3_receiver.
namespace App
{
    public class Fixture
    {
        public int Value;
        public int Create() { return 1; }
    }

    public class Consumer
    {
        public int Run(Fixture fixture)
        {
            Fixture local = fixture;
            var inferred = fixture;
            return fixture.Create() + local.Value + inferred.Create();
        }
    }
}
