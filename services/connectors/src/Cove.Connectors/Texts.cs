namespace Cove.Connectors;

/// <summary>
/// UI language of the messages the connectors return ("de" or "en"). The core
/// sends <c>system.setLanguage</c> before its first request and after every change;
/// until then the texts are German.
/// </summary>
public static class Texts
{
    private static volatile bool _english;

    public static string Language => _english ? "en" : "de";

    /// <summary>Applies a language; returns false for anything but "de" and "en".</summary>
    public static bool SetLanguage(string language)
    {
        if (language is not ("de" or "en"))
        {
            return false;
        }

        _english = language == "en";
        return true;
    }

    /// <summary>The text in the current language.</summary>
    public static string T(string de, string en) => _english ? en : de;
}
