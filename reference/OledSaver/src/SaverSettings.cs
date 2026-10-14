using System;
using Microsoft.Win32;

namespace OledSaver;

/// <summary>
/// Visual theme of the screen saver. Values are persisted; do not renumber.
/// </summary>
internal enum SaverTheme
{
    /// <summary>
    /// Milva's circuit eye routing PCB traces and signals across the screen, with the "milva" wordmark.
    /// </summary>
    Milvasoft = 0,

    /// <summary>
    /// Colour shifting linked points with a large clock.
    /// </summary>
    Constellation = 1,
}

/// <summary>
/// User settings, persisted under HKCU\Software\OledSaver.
/// </summary>
internal sealed class SaverSettings
{
    private const string RegistryPath = @"Software\OledSaver";

    public const SaverTheme DefaultTheme = SaverTheme.Milvasoft;
    public const bool DefaultShowClock = true;
    public const int DefaultBrightness = 60;
    public const int DefaultDensity = 110;
    public const int DefaultSpeed = 5;

    public const int MinBrightness = 10;
    public const int MaxBrightness = 100;
    public const int MinDensity = 20;
    public const int MaxDensity = 200;
    public const int MinSpeed = 1;
    public const int MaxSpeed = 10;

    /// <summary>
    /// Visual theme.
    /// </summary>
    public SaverTheme Theme { get; set; } = DefaultTheme;

    /// <summary>
    /// Draws a drifting clock and date.
    /// </summary>
    public bool ShowClock { get; set; } = DefaultShowClock;

    /// <summary>
    /// Peak brightness of everything drawn, in percent.
    /// </summary>
    public int Brightness { get; set; } = DefaultBrightness;

    /// <summary>
    /// Number of points per 1920x1080 worth of screen area.
    /// </summary>
    public int Density { get; set; } = DefaultDensity;

    /// <summary>
    /// Drift speed, 1 (slowest) to 10.
    /// </summary>
    public int Speed { get; set; } = DefaultSpeed;

    /// <summary>
    /// Reads the settings, falling back to defaults for anything missing or out of range.
    /// </summary>
    public static SaverSettings Load()
    {
        var settings = new SaverSettings();

        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(RegistryPath);

            if (key == null)
                return settings;

            settings.Theme = ReadTheme(key);
            settings.ShowClock = ReadInt(key, nameof(ShowClock), DefaultShowClock ? 1 : 0, 0, 1) == 1;
            settings.Brightness = ReadInt(key, nameof(Brightness), DefaultBrightness, MinBrightness, MaxBrightness);
            settings.Density = ReadInt(key, nameof(Density), DefaultDensity, MinDensity, MaxDensity);
            settings.Speed = ReadInt(key, nameof(Speed), DefaultSpeed, MinSpeed, MaxSpeed);
        }
        catch (Exception)
        {
            // Unreadable registry must never stop the screen saver; defaults are fine.
        }

        return settings;
    }

    /// <summary>
    /// Writes the settings to the registry.
    /// </summary>
    public void Save()
    {
        using var key = Registry.CurrentUser.CreateSubKey(RegistryPath);

        key.SetValue(nameof(Theme), (int)Theme, RegistryValueKind.DWord);
        key.SetValue(nameof(ShowClock), ShowClock ? 1 : 0, RegistryValueKind.DWord);
        key.SetValue(nameof(Brightness), Brightness, RegistryValueKind.DWord);
        key.SetValue(nameof(Density), Density, RegistryValueKind.DWord);
        key.SetValue(nameof(Speed), Speed, RegistryValueKind.DWord);
    }

    private static SaverTheme ReadTheme(RegistryKey key)
    {
        var value = key.GetValue(nameof(Theme)) is int number ? (SaverTheme)number : DefaultTheme;

        return Enum.IsDefined(typeof(SaverTheme), value) ? value : DefaultTheme;
    }

    private static int ReadInt(RegistryKey key, string name, int fallback, int min, int max)
    {
        var value = key.GetValue(name) is int number ? number : fallback;

        return Math.Max(min, Math.Min(max, value));
    }
}
