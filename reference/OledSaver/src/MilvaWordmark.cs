using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Text;

namespace OledSaver;

/// <summary>
/// The lowercase "milva" wordmark as vector paths: heavy letters, a dotless i, and the brand's yellow square dot
/// sitting at ascender height with its top-left corner rounded, as in the logo. Origin is the block's top-left.
/// </summary>
internal sealed class MilvaWordmark : IDisposable
{
    private MilvaWordmark(GraphicsPath letters, GraphicsPath dot, SizeF size)
    {
        Letters = letters;
        Dot = dot;
        Size = size;
    }

    /// <summary>
    /// Letter outlines, drawn in the neutral colour.
    /// </summary>
    public GraphicsPath Letters { get; }

    /// <summary>
    /// The i dot, drawn in brand yellow.
    /// </summary>
    public GraphicsPath Dot { get; }

    public SizeF Size { get; }

    /// <summary>
    /// Builds the wordmark with letters of <paramref name="emSize"/> pixels.
    /// </summary>
    public static MilvaWordmark Create(float emSize)
    {
        var family = ResolveFamily(out var style);

        try
        {
            using var format = (StringFormat)StringFormat.GenericTypographic.Clone();
            using var m = CreateGlyphs("m", family, style, emSize, format);
            using var i = CreateGlyphs("ı", family, style, emSize, format);
            using var rest = CreateGlyphs("lva", family, style, emSize, format);

            var gap = emSize * 0.045f;
            var mBounds = m.GetBounds();
            var iBounds = i.GetBounds();
            var restBounds = rest.GetBounds();

            // Lay glyphs out by their ink bounds with a tight, even gap, like the logo's tracking.
            var iLeft = mBounds.Width + gap;
            var restLeft = iLeft + iBounds.Width + gap;

            MoveHorizontally(m, -mBounds.Left);
            MoveHorizontally(i, iLeft - iBounds.Left);
            MoveHorizontally(rest, restLeft - restBounds.Left);

            iBounds = i.GetBounds();
            restBounds = rest.GetBounds();

            // Dot: as wide as the stem, top aligned with the "l" ascender, a small gap above the stem.
            var dotWidth = iBounds.Width;
            var dotGap = dotWidth * 0.33f;
            var dotHeight = Math.Max(dotWidth * 0.6f, iBounds.Top - dotGap - restBounds.Top);
            var dot = CreateDot(iBounds.Left, iBounds.Top - dotGap - dotHeight, dotWidth, dotHeight);
            var letters = new GraphicsPath();

            letters.AddPath(m, false);
            letters.AddPath(i, false);
            letters.AddPath(rest, false);

            // Normalise so the block's top-left is (0, 0).
            var letterBounds = letters.GetBounds();
            var top = Math.Min(letterBounds.Top, dot.GetBounds().Top);

            using (var normalise = new Matrix())
            {
                normalise.Translate(-letterBounds.Left, -top);
                letters.Transform(normalise);
                dot.Transform(normalise);
            }

            return new MilvaWordmark(letters, dot, new SizeF(letterBounds.Width, letterBounds.Bottom - top));
        }
        finally
        {
            family.Dispose();
        }
    }

    public void Dispose()
    {
        Letters.Dispose();
        Dot.Dispose();
    }

    private static GraphicsPath CreateGlyphs(string text, FontFamily family, FontStyle style, float emSize, StringFormat format)
    {
        var path = new GraphicsPath();

        path.AddString(text, family, (int)style, emSize, PointF.Empty, format);

        return path;
    }

    private static void MoveHorizontally(GraphicsPath path, float offset)
    {
        using var shift = new Matrix();

        shift.Translate(offset, 0);
        path.Transform(shift);
    }

    private static GraphicsPath CreateDot(float x, float y, float width, float height)
    {
        var radius = width * 0.45f;
        var path = new GraphicsPath();

        path.AddArc(x, y, radius * 2, radius * 2, 180, 90);
        path.AddLine(x + radius, y, x + width, y);
        path.AddLine(x + width, y, x + width, y + height);
        path.AddLine(x + width, y + height, x, y + height);
        path.CloseFigure();

        return path;
    }

    /// <summary>
    /// Heaviest available sans: Segoe UI Black on Windows, then Arial Black, then any bold sans.
    /// </summary>
    private static FontFamily ResolveFamily(out FontStyle style)
    {
        foreach (var name in new[] { "Segoe UI Black", "Arial Black" })
        {
            try
            {
                var family = new FontFamily(name);

                if (family.Name == name && family.IsStyleAvailable(FontStyle.Regular))
                {
                    style = FontStyle.Regular;
                    return family;
                }

                family.Dispose();
            }
            catch (ArgumentException)
            {
                // Not installed.
            }
        }

        style = FontStyle.Bold;
        return new FontFamily(GenericFontFamilies.SansSerif);
    }
}
