using System;
using System.Drawing;
using System.Drawing.Drawing2D;

namespace OledSaver;

/// <summary>
/// Base for OLED friendly scenes: pure black background, time keeping and shared drawing helpers.
/// Derived scenes must never leave anything at a fixed position for long.
/// </summary>
internal abstract class Scene : IDisposable
{
    private const double FadeInSeconds = 1.5;

    /// <summary>
    /// Creates a scene. <paramref name="index"/> makes each monitor's scene diverge from the others.
    /// </summary>
    protected Scene(SaverSettings settings, int index)
    {
        Settings = settings;
        Random = new Random(unchecked(Environment.TickCount ^ ((index + 1) * 7919)) & int.MaxValue);
        Phase = index * 2.1 + Random.NextDouble() * Math.PI;
    }

    protected SaverSettings Settings { get; }

    protected Random Random { get; }

    /// <summary>
    /// Per scene random offset used to desynchronise motion between monitors.
    /// </summary>
    protected double Phase { get; }

    protected int Width { get; private set; }

    protected int Height { get; private set; }

    /// <summary>
    /// Scale factor relative to a 1080 pixel tall screen.
    /// </summary>
    protected float Unit { get; private set; }

    /// <summary>
    /// Seconds since the scene started.
    /// </summary>
    protected double Time { get; private set; }

    /// <summary>
    /// 0 → 1 over the first moments so the saver fades in instead of popping.
    /// </summary>
    protected float Fade => (float)Math.Min(1.0, Time / FadeInSeconds);

    /// <summary>
    /// User brightness setting as 0..1.
    /// </summary>
    protected float Brightness => Settings.Brightness / 100f;

    /// <summary>
    /// User speed setting mapped around 1.0 (default speed).
    /// </summary>
    protected float SpeedFactor => 0.5f + Settings.Speed * 0.1f;

    /// <summary>
    /// Creates the scene for the theme selected in <paramref name="settings"/>.
    /// </summary>
    public static Scene Create(SaverSettings settings, int index) => settings.Theme switch
    {
        SaverTheme.Constellation => new ConstellationScene(settings, index),
        _ => new MilvaScene(settings, index),
    };

    /// <summary>
    /// Rebuilds size dependent state for a new drawing surface size.
    /// </summary>
    public void Resize(int width, int height)
    {
        if (width <= 0 || height <= 0 || (width == Width && height == Height))
            return;

        Width = width;
        Height = height;
        Unit = Math.Max(0.15f, Math.Min(width, height) / 1080f);

        OnResized();
    }

    /// <summary>
    /// Advances the simulation by <paramref name="deltaSeconds"/>.
    /// </summary>
    public virtual void Update(double deltaSeconds) => Time += deltaSeconds;

    /// <summary>
    /// Draws the current frame.
    /// </summary>
    public void Draw(Graphics graphics)
    {
        graphics.Clear(Color.Black);

        if (Width <= 0 || Height <= 0)
            return;

        graphics.SmoothingMode = SmoothingMode.AntiAlias;

        Render(graphics);
    }

    public void Dispose()
    {
        Dispose(true);
        GC.SuppressFinalize(this);
    }

    /// <summary>
    /// Draws everything on top of the already cleared, black surface.
    /// </summary>
    protected abstract void Render(Graphics graphics);

    /// <summary>
    /// Called after the surface size changed.
    /// </summary>
    protected virtual void OnResized()
    {
    }

    protected virtual void Dispose(bool disposing)
    {
    }

    /// <summary>
    /// Top-left of a <paramref name="blockWidth"/> x <paramref name="blockHeight"/> block wandering over the whole screen
    /// on a Lissajous path with long, unrelated periods, so it never parks anywhere.
    /// </summary>
    protected PointF Wander(double periodX, double periodY, double phase, float blockWidth, float blockHeight, float margin = 0)
    {
        var u = 0.5 + 0.5 * Math.Sin(Time * 2 * Math.PI / periodX + phase);
        var v = 0.5 + 0.5 * Math.Sin(Time * 2 * Math.PI / periodY + phase * 1.7);

        return new PointF(margin + (float)(u * Math.Max(0, Width - 2 * margin - blockWidth)),
                          margin + (float)(v * Math.Max(0, Height - 2 * margin - blockHeight)));
    }

    /// <summary>
    /// True when the point is on screen, give or take <paramref name="slack"/> pixels.
    /// </summary>
    protected bool IsVisible(float x, float y, float slack = 10) => x >= -slack && x <= Width + slack && y >= -slack && y <= Height + slack;

    /// <summary>
    /// <paramref name="color"/> dimmed to <paramref name="value"/> (0..1) with the given opacity.
    /// </summary>
    protected static Color Dim(Color color, float value, float alpha)
        => Color.FromArgb(ToByte(alpha), ToByte(color.R / 255.0 * value), ToByte(color.G / 255.0 * value), ToByte(color.B / 255.0 * value));

    protected static Color FromHsv(double hue, double saturation, double value, float alpha)
    {
        hue = ((hue % 360) + 360) % 360;

        var chroma = value * saturation;
        var x = chroma * (1 - Math.Abs(hue / 60 % 2 - 1));
        var m = value - chroma;

        var (r, g, b) = (int)(hue / 60) switch
        {
            0 => (chroma, x, 0.0),
            1 => (x, chroma, 0.0),
            2 => (0.0, chroma, x),
            3 => (0.0, x, chroma),
            4 => (x, 0.0, chroma),
            _ => (chroma, 0.0, x),
        };

        return Color.FromArgb(ToByte(alpha), ToByte(r + m), ToByte(g + m), ToByte(b + m));
    }

    private static int ToByte(double value) => Math.Max(0, Math.Min(255, (int)Math.Round(value * 255)));
}
