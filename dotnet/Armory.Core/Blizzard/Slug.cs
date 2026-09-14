using System.Text;

namespace Armory.Blizzard;

public static class Slug
{
    /// <summary>
    /// A realm name as the profile endpoints address it: lowercased, runs of
    /// anything but letters and digits become one dash, and apostrophes
    /// vanish rather than becoming separators. Zul'jin is <c>zuljin</c>.
    /// </summary>
    public static string RealmSlug(string name)
    {
        var slug = new StringBuilder(name.Length);
        var previousDash = false;
        foreach (var character in name)
        {
            if (char.IsAsciiLetterOrDigit(character))
            {
                slug.Append(char.ToLowerInvariant(character));
                previousDash = false;
            }
            else if (character == '\'')
            {
                continue;
            }
            else if (!previousDash && slug.Length > 0)
            {
                slug.Append('-');
                previousDash = true;
            }
        }
        while (slug.Length > 0 && slug[^1] == '-')
        {
            slug.Length--;
        }
        return slug.ToString();
    }
}
