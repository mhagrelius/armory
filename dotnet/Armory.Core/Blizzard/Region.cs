using System.Text.Json;
using System.Text.Json.Serialization;

namespace Armory.Blizzard;

/// <summary>
/// Which of Blizzard's regions an account lives in. China is deliberately
/// absent: it runs on a different host under a different operator and has
/// never been production-ready for third parties.
/// </summary>
[JsonConverter(typeof(RegionConverter))]
public enum Region
{
    Us,
    Eu,
    Kr,
    Tw,
}

public static class RegionExtensions
{
    public static IReadOnlyList<Region> All { get; } = [Region.Us, Region.Eu, Region.Kr, Region.Tw];

    /// <summary>The two-letter code, which is also the namespace suffix.</summary>
    public static string Code(this Region region) => region switch
    {
        Region.Us => "us",
        Region.Eu => "eu",
        Region.Kr => "kr",
        Region.Tw => "tw",
        _ => "us",
    };

    public static string Label(this Region region) => region switch
    {
        Region.Us => "Americas",
        Region.Eu => "Europe",
        Region.Kr => "Korea",
        Region.Tw => "Taiwan",
        _ => "Americas",
    };

    public static Region? FromCode(string code) =>
        All.Cast<Region?>().FirstOrDefault(region => string.Equals(region!.Value.Code(), code, StringComparison.OrdinalIgnoreCase));

    /// <summary>The API host.</summary>
    public static string ApiHost(this Region region) => $"https://{region.Code()}.api.blizzard.com";
}

/// <summary>The two-letter code, lowercase, which is how the Rust settings file spells a region.</summary>
public sealed class RegionConverter : JsonConverter<Region>
{
    public override Region Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options) =>
        RegionExtensions.FromCode(reader.GetString() ?? "") ?? throw new JsonException("not a region");

    public override void Write(Utf8JsonWriter writer, Region value, JsonSerializerOptions options) =>
        writer.WriteStringValue(value.Code());
}
