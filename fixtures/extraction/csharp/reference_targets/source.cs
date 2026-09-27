class ReferenceTargets
{
    static void Target() { }

    static void Caller()
    {
        {
            int Target = 1;
        }
        Target();
    }
}
