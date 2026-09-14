using Xunit;

namespace Armory.Tests;

public sealed class ResultTests
{
    [Fact(DisplayName = "an error carries through a chain without being looked at")]
    public void An_error_carries_through_a_chain()
    {
        var failed = Result<int, string>.Err("no");
        var chained = failed.Map(n => n + 1).Then(n => Result<string, string>.Ok(n.ToString()));
        Assert.False(chained.IsOk);
        Assert.Equal("no", chained.Error);
    }

    [Fact(DisplayName = "a value maps and chains")]
    public void A_value_maps_and_chains()
    {
        var ok = Result<int, string>.Ok(1).Map(n => n + 1).Then(n => Result<string, string>.Ok($"{n}"));
        Assert.Equal("2", ok.Value);
    }
}
