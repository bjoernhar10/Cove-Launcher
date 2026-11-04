using System.Text;

namespace Cove.Connectors.Local;

/// <summary>Valve KeyValues text format (libraryfolders.vdf, appmanifest_*.acf). Keys are case-insensitive.</summary>
public sealed class KeyValues
{
    public Dictionary<string, KeyValues> Children { get; } = new(StringComparer.OrdinalIgnoreCase);

    public Dictionary<string, string> Values { get; } = new(StringComparer.OrdinalIgnoreCase);

    public KeyValues? this[string key] => Children.GetValueOrDefault(key);

    public string? Get(string key) => Values.GetValueOrDefault(key);

    public static KeyValues Parse(string text)
    {
        var tokens = Tokenize(text).GetEnumerator();
        var root = new KeyValues();
        ParseBlock(tokens, root, topLevel: true);
        return root;
    }

    private static void ParseBlock(IEnumerator<string> tokens, KeyValues target, bool topLevel)
    {
        while (tokens.MoveNext())
        {
            var key = tokens.Current;
            if (key == "}")
            {
                if (topLevel)
                {
                    throw new FormatException("unexpected }");
                }

                return;
            }

            if (!tokens.MoveNext())
            {
                throw new FormatException($"missing value for {key}");
            }

            if (tokens.Current == "{")
            {
                var child = new KeyValues();
                ParseBlock(tokens, child, topLevel: false);
                target.Children[key] = child;
            }
            else
            {
                target.Values[key] = tokens.Current;
            }
        }

        if (!topLevel)
        {
            throw new FormatException("missing }");
        }
    }

    private static IEnumerable<string> Tokenize(string text)
    {
        var i = 0;
        while (i < text.Length)
        {
            var c = text[i];
            if (char.IsWhiteSpace(c))
            {
                i++;
            }
            else if (c == '/' && i + 1 < text.Length && text[i + 1] == '/')
            {
                while (i < text.Length && text[i] != '\n')
                {
                    i++;
                }
            }
            else if (c is '{' or '}')
            {
                i++;
                yield return c.ToString();
            }
            else if (c == '"')
            {
                var sb = new StringBuilder();
                i++;
                while (i < text.Length && text[i] != '"')
                {
                    if (text[i] == '\\' && i + 1 < text.Length)
                    {
                        i++;
                        sb.Append(text[i] switch { 'n' => '\n', 't' => '\t', _ => text[i] });
                    }
                    else
                    {
                        sb.Append(text[i]);
                    }

                    i++;
                }

                i++;
                yield return sb.ToString();
            }
            else
            {
                var start = i;
                while (i < text.Length && !char.IsWhiteSpace(text[i]) && text[i] is not ('{' or '}' or '"'))
                {
                    i++;
                }

                yield return text[start..i];
            }
        }
    }
}
