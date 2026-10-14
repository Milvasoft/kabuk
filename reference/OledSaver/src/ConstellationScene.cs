using System;
using System.Drawing;
using System.Drawing.Text;
using System.Globalization;

namespace OledSaver;

/// <summary>
/// "Takımyıldız" theme: softly linked, slowly colour shifting points and a wandering clock.
/// </summary>
internal sealed class ConstellationScene : Scene
{
    private const double HueCycleSeconds = 240;
    private const double HueSpread = 70;
    private const double ClockPeriodX = 173;
    private const double ClockPeriodY = 109;
    private const int HueBands = 12;
    private const int AlphaLevels = 16;
    private const float MaxLinkAlpha = 0.7f;
    private const float NodeAlpha = 0.85f;
    private const float ClockAlpha = 0.55f;
    private const float BaseLinkDistance = 170f;
    private const int MinNodes = 12;
    private const int MaxNodes = 900;

    private readonly double _clockPhase;
    private readonly CultureInfo _culture = CultureInfo.CurrentCulture;

    private Node[] _nodes = [];
    private float _linkDistance;
    private float _padding;
    private Font _timeFont;
    private Font _dateFont;

    private struct Node
    {
        public float X;
        public float Y;
        public float Angle;
        public float Speed;
        public float Size;
    }

    public ConstellationScene(SaverSettings settings, int index) : base(settings, index)
    {
        _clockPhase = Random.NextDouble() * Math.PI * 2;
    }

    public override void Update(double deltaSeconds)
    {
        base.Update(deltaSeconds);

        var speedPixels = Unit * (6 + Settings.Speed * 5);
        var minX = -_padding;
        var maxX = Width + _padding;
        var minY = -_padding;
        var maxY = Height + _padding;
        var spanX = maxX - minX;
        var spanY = maxY - minY;

        for (var i = 0; i < _nodes.Length; i++)
        {
            ref var node = ref _nodes[i];

            // A slowly changing flow field bends every path so the motion never repeats.
            var field = Math.Sin(node.X / (Unit * 260) + Time * 0.07 + Phase)
                        + Math.Cos(node.Y / (Unit * 210) - Time * 0.05);

            node.Angle += (float)(field * 0.35 * deltaSeconds);
            node.X += (float)(Math.Cos(node.Angle) * node.Speed * speedPixels * deltaSeconds);
            node.Y += (float)(Math.Sin(node.Angle) * node.Speed * speedPixels * deltaSeconds);

            if (node.X < minX)
                node.X += spanX;
            else if (node.X > maxX)
                node.X -= spanX;

            if (node.Y < minY)
                node.Y += spanY;
            else if (node.Y > maxY)
                node.Y -= spanY;
        }
    }

    protected override void OnResized()
    {
        _linkDistance = BaseLinkDistance * Unit;
        _padding = _linkDistance;

        var areaFactor = (double)Width * Height / (1920.0 * 1080.0);
        var count = Math.Max(MinNodes, Math.Min(MaxNodes, (int)Math.Round(Settings.Density * areaFactor)));
        var nodes = new Node[count];

        for (var i = 0; i < count; i++)
        {
            nodes[i] = new Node
            {
                X = (float)(Random.NextDouble() * (Width + 2 * _padding) - _padding),
                Y = (float)(Random.NextDouble() * (Height + 2 * _padding) - _padding),
                Angle = (float)(Random.NextDouble() * Math.PI * 2),
                Speed = (float)(0.6 + Random.NextDouble() * 0.8),
                Size = (float)(2.2 + Random.NextDouble() * 2.4),
            };
        }

        _nodes = nodes;

        _timeFont?.Dispose();
        _dateFont?.Dispose();
        _timeFont = new Font("Segoe UI Light", Math.Max(6f, Height * 0.12f), FontStyle.Regular, GraphicsUnit.Pixel);
        _dateFont = new Font("Segoe UI", Math.Max(5f, Height * 0.028f), FontStyle.Regular, GraphicsUnit.Pixel);
    }

    protected override void Render(Graphics graphics)
    {
        if (_nodes.Length == 0)
            return;

        var baseHue = (Time * 360 / HueCycleSeconds + Phase * 40) % 360;

        DrawLinks(graphics, baseHue);
        DrawNodes(graphics, baseHue);

        if (Settings.ShowClock)
            DrawClock(graphics, baseHue);
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _timeFont?.Dispose();
            _dateFont?.Dispose();
        }

        base.Dispose(disposing);
    }

    private void DrawLinks(Graphics graphics, double baseHue)
    {
        var nodes = _nodes;
        var pens = new Pen[HueBands * AlphaLevels];
        var lineWidth = Math.Max(1f, 1.2f * Unit);
        var maxDistanceSquared = _linkDistance * _linkDistance;

        try
        {
            for (var i = 0; i < nodes.Length; i++)
            {
                var a = nodes[i];

                for (var j = i + 1; j < nodes.Length; j++)
                {
                    var b = nodes[j];
                    var dx = a.X - b.X;
                    var dy = a.Y - b.Y;
                    var distanceSquared = dx * dx + dy * dy;

                    if (distanceSquared >= maxDistanceSquared)
                        continue;

                    var closeness = 1f - (float)Math.Sqrt(distanceSquared) / _linkDistance;
                    var level = (int)(closeness * (AlphaLevels - 1) + 0.5f);

                    if (level <= 0)
                        continue;

                    var band = HueBand((a.X + b.X) * 0.5f);
                    var slot = band * AlphaLevels + level;
                    var pen = pens[slot];

                    if (pen == null)
                    {
                        var alpha = (float)level / (AlphaLevels - 1) * MaxLinkAlpha * Fade;
                        pen = pens[slot] = new Pen(FromHsv(BandHue(baseHue, band), 0.75, Brightness, alpha), lineWidth);
                    }

                    graphics.DrawLine(pen, a.X, a.Y, b.X, b.Y);
                }
            }
        }
        finally
        {
            foreach (var pen in pens)
                pen?.Dispose();
        }
    }

    private void DrawNodes(Graphics graphics, double baseHue)
    {
        var brushes = new SolidBrush[HueBands];

        try
        {
            foreach (var node in _nodes)
            {
                if (!IsVisible(node.X, node.Y))
                    continue;

                var band = HueBand(node.X);
                var brush = brushes[band] ??= new SolidBrush(FromHsv(BandHue(baseHue, band), 0.55, Brightness, NodeAlpha * Fade));
                var size = Math.Max(1f, node.Size * Unit);

                graphics.FillEllipse(brush, node.X - size / 2, node.Y - size / 2, size, size);
            }
        }
        finally
        {
            foreach (var brush in brushes)
                brush?.Dispose();
        }
    }

    private void DrawClock(Graphics graphics, double baseHue)
    {
        var now = DateTime.Now;
        var time = now.ToString("HH:mm", _culture);
        var date = now.ToString("d MMMM dddd", _culture);

        graphics.TextRenderingHint = TextRenderingHint.AntiAlias;

        var timeSize = graphics.MeasureString(time, _timeFont);
        var dateSize = graphics.MeasureString(date, _dateFont);
        var blockWidth = Math.Max(timeSize.Width, dateSize.Width);
        var dateOffset = timeSize.Height * 0.92f;
        var position = Wander(ClockPeriodX, ClockPeriodY, _clockPhase, blockWidth, dateOffset + dateSize.Height);

        using var brush = new SolidBrush(FromHsv(baseHue + 150, 0.3, Brightness, ClockAlpha * Fade));

        graphics.DrawString(time, _timeFont, brush, position.X + (blockWidth - timeSize.Width) / 2, position.Y);
        graphics.DrawString(date, _dateFont, brush, position.X + (blockWidth - dateSize.Width) / 2, position.Y + dateOffset);
    }

    private int HueBand(float x)
    {
        var position = (x + _padding) / (Width + 2 * _padding);
        var band = (int)(position * HueBands);

        return Math.Max(0, Math.Min(HueBands - 1, band));
    }

    private static double BandHue(double baseHue, int band) => baseHue + band * HueSpread / (HueBands - 1);
}
