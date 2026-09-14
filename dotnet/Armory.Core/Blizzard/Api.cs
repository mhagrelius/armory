using System.Globalization;
using System.Text;

namespace Armory.Blizzard;

/// <summary>
/// Which slice of the API a request is addressed to. Blizzard versions these,
/// but the bare form is stable and is what is sent: pinning a build would
/// break the application on every patch day.
/// </summary>
public enum Namespace
{
    /// <summary>Catalogue data that changes on patch days.</summary>
    Static,

    /// <summary>Data that changes hourly or faster: realms, auctions, the token price.</summary>
    Dynamic,

    /// <summary>This account and its characters.</summary>
    Profile,
}

public static class RegionLocales
{
    /// <summary>The default locale for the region, used when the user has not chosen.</summary>
    public static string DefaultLocale(this Region region) => region switch
    {
        Region.Us => "en_US",
        Region.Eu => "en_GB",
        Region.Kr => "ko_KR",
        Region.Tw => "zh_TW",
        _ => "en_US",
    };
}

public static class Api
{
    public static string Qualified(this Namespace ns, Region region) => (ns switch
    {
        Namespace.Static => "static",
        Namespace.Dynamic => "dynamic",
        _ => "profile",
    }) + "-" + region.Code();

    /// <summary>
    /// Build an API URL with its namespace and locale already attached. Every
    /// call needs both, and forgetting the namespace produces a 404 that reads
    /// like a missing character rather than a missing parameter.
    /// </summary>
    public static string Url(Region region, Namespace ns, string path, params (string Name, string Value)[] extra)
    {
        var url = new StringBuilder();
        url.Append(region.ApiHost()).Append(path)
            .Append("?namespace=").Append(ns.Qualified(region))
            .Append("&locale=").Append(region.DefaultLocale());
        foreach (var (name, value) in extra)
        {
            url.Append('&').Append(name).Append('=').Append(Encode(value));
        }
        return url.ToString();
    }

    /// <summary>Percent-encode one query parameter value.</summary>
    public static string Encode(string value)
    {
        var encoded = new StringBuilder(value.Length);
        foreach (var b in Encoding.UTF8.GetBytes(value))
        {
            if (b is (>= (byte)'A' and <= (byte)'Z') or (>= (byte)'a' and <= (byte)'z') or (>= (byte)'0' and <= (byte)'9') or (byte)'-' or (byte)'_' or (byte)'.' or (byte)'~')
            {
                encoded.Append((char)b);
            }
            else if (b == (byte)' ')
            {
                encoded.Append("%20");
            }
            else
            {
                encoded.Append('%').Append(b.ToString("X2", CultureInfo.InvariantCulture));
            }
        }
        return encoded.ToString();
    }

    /// <summary>Percent-decode one query parameter value, with <c>+</c> as a space.</summary>
    public static string Decode(string value)
    {
        var bytes = Encoding.UTF8.GetBytes(value);
        var decoded = new List<byte>(bytes.Length);
        var index = 0;
        while (index < bytes.Length)
        {
            var b = bytes[index];
            if (b == (byte)'%' && index + 2 < bytes.Length
                && byte.TryParse(Encoding.ASCII.GetString(bytes, index + 1, 2), NumberStyles.HexNumber, CultureInfo.InvariantCulture, out var escaped))
            {
                decoded.Add(escaped);
                index += 3;
            }
            else if (b == (byte)'+')
            {
                decoded.Add((byte)' ');
                index++;
            }
            else
            {
                decoded.Add(b);
                index++;
            }
        }
        return Encoding.UTF8.GetString(decoded.ToArray());
    }
}
