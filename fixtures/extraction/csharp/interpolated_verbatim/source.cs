namespace Issue19;

public sealed class InterpolatedVerbatim
{
    public string DollarAt(string value)
    {
        return @$"prefix ""{value}""";
    }

    public string AtDollar(string value)
    {
        return $@"prefix ""{value}""";
    }

    public string SubsequentMethod() => "recovered";
}
