namespace App
{
    public class Consumer
    {
        public int Run()
        {
            var made = Fixture.Create();
            var shade = Color.Red;
            return made + Limits.Max;
        }
    }
}
