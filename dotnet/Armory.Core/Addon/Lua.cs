using System.Globalization;
using System.Text;

namespace Armory.Addon;

/// <summary>
/// A value out of a SavedVariables file.
/// </summary>
/// <remarks>
/// Lua tables are one structure with two halves: a positional array part and
/// a keyed hash part. Keeping them apart matches how the file is written and
/// how it is read back.
/// </remarks>
public abstract record LuaValue
{
    public sealed record Nil : LuaValue
    {
        public static Nil Value { get; } = new();
    }

    public sealed record Bool(bool Flag) : LuaValue;

    public sealed record Number(double Value) : LuaValue;

    public sealed record Str(string Text) : LuaValue;

    public sealed record Table(List<LuaValue> Array, SortedDictionary<LuaKey, LuaValue> Map) : LuaValue
    {
        public bool Equals(Table? other) =>
            other is not null && Array.SequenceEqual(other.Array) && Map.SequenceEqual(other.Map);

        public override int GetHashCode() => Array.Count ^ Map.Count;
    }

    public string? AsStr() => this is Str text ? text.Text : null;

    public double? AsDouble() => this is Number number ? number.Value : null;

    /// <summary>A whole number, or null for anything else. The Rust reads these as <c>u32</c>.</summary>
    public long? AsInteger() => this is Number number ? (long)number.Value : null;

    public bool? AsBool() => this is Bool flag ? flag.Flag : null;

    /// <summary>Look a key up in the hash part of a table, by its text, whether it was a string or a number.</summary>
    public LuaValue? Get(string key) =>
        this is Table table
            ? table.Map.GetValueOrDefault(LuaKey.String(key)) ?? table.Map.GetValueOrDefault(LuaKey.Numeric(key))
            : null;

    /// <summary>The hash part, for walking a table whose keys are data.</summary>
    public IEnumerable<KeyValuePair<LuaKey, LuaValue>> Entries() =>
        this is Table table ? table.Map : [];

    /// <summary>The array part.</summary>
    public IReadOnlyList<LuaValue> Items() => this is Table table ? table.Array : [];
}

/// <summary>
/// A table key. Lua allows numbers and strings, and SavedVariables uses both:
/// achievement ids arrive as numbers, character names as strings. A number is
/// kept as its text so keys order and compare without float equality.
/// </summary>
public readonly record struct LuaKey(bool IsNumber, string Text) : IComparable<LuaKey>
{
    public static LuaKey Numeric(string text) => new(true, text);

    public static LuaKey String(string text) => new(false, text);

    public string AsStr() => Text;

    public long? AsInteger() =>
        long.TryParse(Text, NumberStyles.Integer, CultureInfo.InvariantCulture, out var number) ? number : null;

    /// <summary>Numbers before strings, then by text, as the Rust's ordered map does.</summary>
    public int CompareTo(LuaKey other)
    {
        if (IsNumber != other.IsNumber)
        {
            return IsNumber ? -1 : 1;
        }
        return string.CompareOrdinal(Text, other.Text);
    }

    public static bool operator <(LuaKey left, LuaKey right) => left.CompareTo(right) < 0;

    public static bool operator >(LuaKey left, LuaKey right) => left.CompareTo(right) > 0;

    public static bool operator <=(LuaKey left, LuaKey right) => left.CompareTo(right) <= 0;

    public static bool operator >=(LuaKey left, LuaKey right) => left.CompareTo(right) >= 0;
}

/// <summary>Why a SavedVariables file could not be read, and where.</summary>
public sealed record LuaParseError(string Message, int At)
{
    public override string ToString() => $"{Message} at byte {At}";
}

/// <summary>
/// Reading the Lua that WoW writes.
/// </summary>
/// <remarks>
/// <para>An addon cannot open a socket or write a file of its own, so
/// SavedVariables is the only way data leaves the game: plain Lua source,
/// written at logout, that something outside has to read.</para>
/// <para><b>This is not a Lua interpreter and must not become one.</b>
/// SavedVariables is a generated file in a narrow shape: assignments of
/// table literals containing numbers, strings, booleans, <c>nil</c>, and more
/// tables. Nothing in it calls a function, and anything that appears to is a
/// file to refuse rather than evaluate. This parses input that a game wrote
/// into a directory an addon manager also writes to.</para>
/// </remarks>
public static class Lua
{
    /// <summary>
    /// How deep a table may nest before the file is refused. Nothing WoW
    /// writes gets near this; the limit exists so a malformed or hostile file
    /// cannot recurse the parser into a stack overflow, which is a crash
    /// rather than an error.
    /// </summary>
    private const int MaxDepth = 128;

    /// <summary>Read a whole SavedVariables file: a sequence of <c>Name = value</c> assignments.</summary>
    public static Result<SortedDictionary<string, LuaValue>, LuaParseError> Parse(string source) =>
        Parse(Encoding.UTF8.GetBytes(source));

    /// <summary>The same, over the file's bytes, which is how a file is read.</summary>
    public static Result<SortedDictionary<string, LuaValue>, LuaParseError> Parse(byte[] source)
    {
        var parser = new Parser(source);
        var globals = new SortedDictionary<string, LuaValue>(StringComparer.Ordinal);
        try
        {
            while (true)
            {
                parser.SkipTrivia();
                if (parser.AtEnd)
                {
                    return Result<SortedDictionary<string, LuaValue>, LuaParseError>.Ok(globals);
                }
                var name = parser.Name();
                parser.SkipTrivia();
                parser.Expect((byte)'=');
                globals[name] = parser.Value();
                parser.SkipTrivia();
                // The writer does not emit statement separators, but a
                // hand-edited file might.
                if (parser.Peek() == (byte)';')
                {
                    parser.Advance();
                }
            }
        }
        catch (Refused refused)
        {
            return Result<SortedDictionary<string, LuaValue>, LuaParseError>.Err(refused.Error);
        }
    }

    /// <summary>Render a number the way a key should read: <c>123</c>, not <c>123.0</c>.</summary>
    internal static string FormatNumber(double number) =>
        number == Math.Floor(number) && Math.Abs(number) < 1e15
            ? ((long)number).ToString(CultureInfo.InvariantCulture)
            : number.ToString("R", CultureInfo.InvariantCulture);

    /// <summary>The parser's way out. Caught once, in <see cref="Parse(byte[])"/>, and turned into the typed error.</summary>
    private sealed class Refused : Exception
    {
        public Refused(LuaParseError error)
            : base(error.ToString())
        {
            Error = error;
        }

        public LuaParseError Error { get; }
    }

    private sealed class Parser
    {
        private readonly byte[] bytes;
        private int at;
        private int depth;

        public Parser(byte[] bytes)
        {
            this.bytes = bytes;
        }

        public bool AtEnd => at >= bytes.Length;

        public byte? Peek() => at < bytes.Length ? bytes[at] : null;

        public void Advance() => at++;

        private Refused Error(string message) => new(new LuaParseError(message, at));

        private bool StartsWith(string text)
        {
            if (at + text.Length > bytes.Length)
            {
                return false;
            }
            for (var i = 0; i < text.Length; i++)
            {
                if (bytes[at + i] != text[i])
                {
                    return false;
                }
            }
            return true;
        }

        /// <summary>Skip whitespace and <c>--</c> comments, including <c>--[[ long ]]</c> ones.</summary>
        public void SkipTrivia()
        {
            while (true)
            {
                while (Peek() is { } b && IsSpace(b))
                {
                    at++;
                }
                if (StartsWith("--"))
                {
                    at += 2;
                    if (StartsWith("[["))
                    {
                        at += 2;
                        while (at < bytes.Length && !StartsWith("]]"))
                        {
                            at++;
                        }
                        at = Math.Min(at + 2, bytes.Length);
                    }
                    else
                    {
                        while (Peek() is { } b && b != (byte)'\n')
                        {
                            at++;
                        }
                    }
                    continue;
                }
                return;
            }
        }

        public void Expect(byte expected)
        {
            if (Peek() != expected)
            {
                throw Error($"expected `{(char)expected}`");
            }
            at++;
        }

        public string Name()
        {
            var start = at;
            while (Peek() is { } b && (IsAlphanumeric(b) || b == (byte)'_'))
            {
                at++;
            }
            if (start == at)
            {
                throw Error("expected a variable name");
            }
            return Encoding.UTF8.GetString(bytes, start, at - start);
        }

        public LuaValue Value()
        {
            SkipTrivia();
            if (Peek() is not { } next)
            {
                throw Error("expected a value");
            }
            if (next == (byte)'{')
            {
                return Table();
            }
            if (next is (byte)'"' or (byte)'\'')
            {
                return new LuaValue.Str(String());
            }
            if (next == (byte)'[' && StartsWith("[["))
            {
                return new LuaValue.Str(LongString());
            }
            if (IsDigit(next) || next is (byte)'-' or (byte)'.')
            {
                return Number();
            }
            return Keyword();
        }

        private LuaValue Keyword()
        {
            if (StartsWith("true"))
            {
                at += 4;
                return new LuaValue.Bool(true);
            }
            if (StartsWith("false"))
            {
                at += 5;
                return new LuaValue.Bool(false);
            }
            if (StartsWith("nil"))
            {
                at += 3;
                return LuaValue.Nil.Value;
            }
            // Anything else here is a function call, a concatenation or an
            // identifier, none of which SavedVariables contains, and none of
            // which this is willing to evaluate.
            throw Error("expected a table, string, number, boolean or nil");
        }

        private LuaValue Number()
        {
            var start = at;
            if (Peek() == (byte)'-')
            {
                at++;
            }
            // Hex, which the writer emits for a few fields.
            if (StartsWith("0x") || StartsWith("0X"))
            {
                at += 2;
                var digits = at;
                while (Peek() is { } hex && IsHexDigit(hex))
                {
                    at++;
                }
                var hexText = Encoding.ASCII.GetString(bytes, digits, at - digits);
                return ulong.TryParse(hexText, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out var hexValue)
                    ? new LuaValue.Number(hexValue)
                    : throw Error("a hex number that will not parse");
            }
            while (Peek() is { } b && (IsDigit(b) || b is (byte)'.' or (byte)'e' or (byte)'E' or (byte)'+'))
            {
                at++;
            }
            // A trailing `-` in an exponent, which the loop above stops short of.
            if (at > 0 && bytes[at - 1] is (byte)'e' or (byte)'E' && Peek() == (byte)'-')
            {
                at++;
                while (Peek() is { } digit && IsDigit(digit))
                {
                    at++;
                }
            }
            var text = Encoding.ASCII.GetString(bytes, start, at - start);
            return double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var number)
                ? new LuaValue.Number(number)
                : throw Error($"`{text}` is not a number");
        }

        private string String()
        {
            var quote = Peek()!.Value;
            at++;
            // Bytes, not chars: the writer escapes anything above ASCII as
            // `\ddd` bytes, and a multi-byte UTF-8 sequence written as three
            // escapes has to reassemble.
            var buffer = new List<byte>();
            while (true)
            {
                switch (Peek())
                {
                    case null:
                        throw Error("a string that never ends");
                    case var b when b == quote:
                        at++;
                        return Encoding.UTF8.GetString(buffer.ToArray());
                    case (byte)'\\':
                        at++;
                        switch (Peek())
                        {
                            case null:
                                throw Error("a string that ends in a backslash");
                            case (byte)'n':
                                buffer.Add((byte)'\n');
                                break;
                            case (byte)'t':
                                buffer.Add((byte)'\t');
                                break;
                            case (byte)'r':
                                buffer.Add((byte)'\r');
                                break;
                            case var digit when IsDigit(digit.Value):
                                {
                                    var start = at;
                                    var count = 0;
                                    while (count < 3 && Peek() is { } d && IsDigit(d))
                                    {
                                        at++;
                                        count++;
                                    }
                                    var text = Encoding.ASCII.GetString(bytes, start, at - start);
                                    if (!byte.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out var escaped))
                                    {
                                        throw Error("a byte escape out of range");
                                    }
                                    buffer.Add(escaped);
                                    continue;
                                }
                            case var other:
                                buffer.Add(other.Value);
                                break;
                        }
                        at++;
                        break;
                    default:
                        {
                            var start = at;
                            while (Peek() is { } b && b != quote && b != (byte)'\\')
                            {
                                at++;
                            }
                            buffer.AddRange(bytes.AsSpan(start, at - start));
                            break;
                        }
                }
            }
        }

        private string LongString()
        {
            at += 2;
            var start = at;
            while (at < bytes.Length && !StartsWith("]]"))
            {
                at++;
            }
            if (at >= bytes.Length)
            {
                throw Error("a long string that never ends");
            }
            var text = Encoding.UTF8.GetString(bytes, start, at - start);
            at += 2;
            return text;
        }

        private LuaValue Table()
        {
            if (depth >= MaxDepth)
            {
                throw Error("a table nested past anything the game writes");
            }
            depth++;
            Expect((byte)'{');

            var array = new List<LuaValue>();
            var map = new SortedDictionary<LuaKey, LuaValue>();

            while (true)
            {
                SkipTrivia();
                switch (Peek())
                {
                    case null:
                        throw Error("a table that never closes");
                    case (byte)'}':
                        at++;
                        depth--;
                        return new LuaValue.Table(array, map);
                    case (byte)',' or (byte)';':
                        at++;
                        break;
                    case (byte)'[':
                        {
                            // `["key"] = value` or `[123] = value`.
                            at++;
                            SkipTrivia();
                            LuaKey key;
                            if (Peek() is (byte)'"' or (byte)'\'')
                            {
                                key = LuaKey.String(String());
                            }
                            else if (Number() is LuaValue.Number number)
                            {
                                key = LuaKey.Numeric(FormatNumber(number.Value));
                            }
                            else
                            {
                                throw Error("a table key that is not a string or number");
                            }
                            SkipTrivia();
                            Expect((byte)']');
                            SkipTrivia();
                            Expect((byte)'=');
                            map[key] = Value();
                            break;
                        }
                    case var b when IsAlphabetic(b.Value) || b == (byte)'_':
                        {
                            // Either `key = value` unquoted, or a bare `nil`,
                            // `true` or `false` sitting in the array part.
                            //
                            // WoW's serializer pads a sparse array with `nil`,
                            // so a table keyed by mount id starting at 6 comes
                            // out as five `nil`s and then the entries. That is
                            // most of a real collections dump, not a corner
                            // case; reading `nil` as an unquoted key and then
                            // demanding `=` fails on every file the game writes.
                            var start = at;
                            var name = Name();
                            SkipTrivia();
                            if (Peek() == (byte)'=')
                            {
                                at++;
                                map[LuaKey.String(name)] = Value();
                            }
                            else
                            {
                                at = start;
                                array.Add(Keyword());
                            }
                            break;
                        }
                    default:
                        // A positional entry.
                        array.Add(Value());
                        break;
                }
            }
        }

        private static bool IsSpace(byte b) => b is (byte)' ' or (byte)'\t' or (byte)'\n' or (byte)'\r' or 0x0B or 0x0C;

        private static bool IsDigit(byte b) => b is >= (byte)'0' and <= (byte)'9';

        private static bool IsHexDigit(byte b) => IsDigit(b) || b is >= (byte)'a' and <= (byte)'f' || b is >= (byte)'A' and <= (byte)'F';

        private static bool IsAlphabetic(byte b) => b is >= (byte)'a' and <= (byte)'z' || b is >= (byte)'A' and <= (byte)'Z';

        private static bool IsAlphanumeric(byte b) => IsAlphabetic(b) || IsDigit(b);
    }
}
