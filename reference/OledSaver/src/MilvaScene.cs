using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Text;
using System.Globalization;

namespace OledSaver;

/// <summary>
/// Milvasoft theme, built around Milva's eye from the logo: a circuit eye whose legs end in PCB pads and vias.
/// The eye opens somewhere on the board, blinks and follows the signals; circuit traces route out of its pads
/// and across the screen on a 45° grid, signals run along them, and everything fades away again so no pixel
/// keeps the same job for long. The "milva" wordmark and the clock wander on top.
/// </summary>
internal sealed class MilvaScene : Scene
{
    private static readonly Color _yellow = Color.FromArgb(0xFB, 0xC4, 0x19);
    private static readonly Color _orange = Color.FromArgb(0xF3, 0x84, 0x24);
    private static readonly Color _grey = Color.FromArgb(0xC0, 0xBF, 0xBE);

    /// <summary>
    /// Grid steps, clockwise from east in screen coordinates (y down): E, SE, S, SW, W, NW, N, NE.
    /// </summary>
    private static readonly Point[] _directions = [new(1, 0), new(1, 1), new(0, 1), new(-1, 1), new(-1, 0), new(-1, -1), new(0, -1), new(1, -1)];

    #region Eye geometry (logo units around the eye centre, traced from the Milvasoft logo)

    private const float BarAngle = -8.1f;
    private const float BodyRadius = 60f;
    private const float PupilRadius = 17f;
    private const float PupilRestX = 23f;
    private const float PupilTravel = 34f;
    private const float LogoStroke = 12f;
    private const float RingStroke = 8f;
    private const float EyeExtent = 168f;

    private static readonly PointF _barStart = new(-109, 15);
    private static readonly PointF _barEnd = new(95, -14);

    private static readonly PointF[][] _legs =
    [
        [new(13, -12), new(35, -69)],
        [new(-48, 6), new(-46, -46), new(-130, -38)],
        [new(-25, 54), new(-53, 122)],
        [new(50, 46), new(117, 54), new(132, 10)],
    ];

    private static readonly EyePad[] _eyePads =
    [
        new(new PointF(35, -69), 18, false, 6),
        new(new PointF(-145, -39), 15, true, 4),
        new(new PointF(-53, 122), 18, false, 2),
        new(new PointF(136, -6), 15, true, 0),
    ];

    #endregion

    #region Tuning

    private const float EyeScale = 1.25f;
    private const float CellSize = 32f;
    private const float BoardTraceWidth = 7f;
    private const float EyeTraceWidth = 9.5f;
    private const float TraceLevel = 0.32f;
    private const float EyeTraceLevel = 0.55f;
    private const float EyeLevel = 0.8f;
    private const float WordmarkLevel = 0.75f;
    private const float ClockLevel = 0.55f;

    private const int MinRun = 2;
    private const int MinBoardCells = 4;
    private const int MinEyeCells = 3;
    private const int BoardTracesPer1080p = 12;
    private const float GrowCellsPerSecond = 16f;
    private const float PulseSpeed = 420f;
    private const float PulseTrail = 90f;
    private const float GlowSeconds = 1.1f;

    private const double TraceLifeMin = 14;
    private const double TraceLifeMax = 32;
    private const double TraceFadeSeconds = 2.5;
    private const double EyeLifeMin = 40;
    private const double EyeLifeMax = 65;
    private const double EyeOpenSeconds = 0.7;
    private const double EyeCloseSeconds = 0.7;
    private const double EyeGapSeconds = 1.2;
    private const double BlinkSeconds = 0.28;

    private const float BrandGapFactor = 0.12f;

    #endregion

    private readonly List<Trace> _traces = [];
    private readonly CultureInfo _culture = CultureInfo.CurrentCulture;

    private int[] _owners = [];
    private int _columns;
    private int _rows;
    private float _cell;
    private PointF _gridOrigin;
    private int _nextTraceId = 1;
    private Eye _eye;
    private double _nextEyeAt;
    private double _nextSpawnAt;
    private MilvaWordmark _wordmark;
    private Font _clockFont;
    private SizeF _brandSize;

    private enum TraceState
    {
        Growing,
        Live,
        Fading,
    }

    /// <summary>
    /// One of the eye's four legs: where its pad is, how it looks and which way a trace leaves it.
    /// </summary>
    private readonly struct EyePad(PointF center, float radius, bool isRing, int direction)
    {
        public PointF Center { get; } = center;

        public float Radius { get; } = radius;

        public bool IsRing { get; } = isRing;

        public int Direction { get; } = direction;
    }

    /// <summary>
    /// A circuit trace routed cell by cell over the board grid.
    /// </summary>
    private sealed class Trace
    {
        public int Id;
        public int EyePadIndex = -1;
        public float Width;
        public bool HasStartPad;
        public bool StartRing;
        public bool EndRing;
        public int TargetCells;
        public int Direction;
        public int Run;
        public double GrowAfter;
        public float Visible;
        public TraceState State;
        public double LiveUntil;
        public double FadeStart;
        public double FadeEnd;
        public double NextPulse;
        public float Pulse = -1f;
        public bool Reverse;
        public float StartGlow;
        public float EndGlow;

        public readonly List<Point> Cells = [];
        public readonly List<PointF> Points = [];
        public readonly List<float> Distances = [];

        public bool FromEye => EyePadIndex >= 0;

        public float Length => Distances.Count == 0 ? 0 : Distances[Distances.Count - 1];

        public void AddPoint(PointF point)
        {
            Distances.Add(Points.Count == 0 ? 0 : Length + Distance(Points[Points.Count - 1], point));
            Points.Add(point);
        }
    }

    /// <summary>
    /// Milva's eye while it is open somewhere on the board.
    /// </summary>
    private sealed class Eye
    {
        public PointF Center;
        public float Rotation;
        public float Scale;
        public double OpenAt;
        public double CloseAt;
        public bool Closing;
        public bool TracesCreated;
        public double NextBlink;
        public double BlinkStart = double.MinValue;
        public float PupilX = PupilRestX;
        public float Flash;
        public readonly float[] PadGlow = new float[4];
        public readonly List<Trace> Traces = [];

        public bool HasBrand;

        /// <summary>
        /// Screen rectangle of the wordmark + clock block standing next to the eye.
        /// </summary>
        public RectangleF Brand;

        public float Radius => EyeExtent * Scale;
    }

    public MilvaScene(SaverSettings settings, int index) : base(settings, index)
    {
    }

    public override void Update(double deltaSeconds)
    {
        base.Update(deltaSeconds);

        if (_owners.Length == 0)
            return;

        UpdateTraces(deltaSeconds);
        UpdateEye(deltaSeconds);

        if (Time >= _nextSpawnAt)
        {
            if (CountBoardTraces() < TargetBoardTraces())
                SpawnBoardTrace();

            _nextSpawnAt = Time + (0.15 + Random.NextDouble() * 0.45) / SpeedFactor;
        }
    }

    protected override void OnResized()
    {
        _cell = Math.Max(4f, CellSize * Unit);
        _columns = (int)(Width / _cell) + 3;
        _rows = (int)(Height / _cell) + 3;
        _owners = new int[_columns * _rows];

        // Random sub-cell offset so the lattice never sits on the same pixels twice.
        _gridOrigin = new PointF((float)(-_cell * Random.NextDouble()), (float)(-_cell * Random.NextDouble()));

        _traces.Clear();
        _eye = null;
        _nextEyeAt = Time + 0.6;
        _nextSpawnAt = Time + 1.2;

        _wordmark?.Dispose();
        _wordmark = MilvaWordmark.Create(Math.Max(8f, Height * 0.11f));

        _clockFont?.Dispose();
        _clockFont = new Font("Segoe UI Semibold", Math.Max(5f, Height * 0.026f), FontStyle.Regular, GraphicsUnit.Pixel);
        _brandSize = MeasureBrand();
    }

    protected override void Render(Graphics graphics)
    {
        DrawTraces(graphics);
        DrawPulses(graphics);
        DrawBrand(graphics);
        DrawEye(graphics);
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _wordmark?.Dispose();
            _clockFont?.Dispose();
        }

        base.Dispose(disposing);
    }

    /// <summary>
    /// Brand yellow/orange accents are small and always moving, so they may run brighter than the rest.
    /// </summary>
    private float Accent => Math.Min(1f, Brightness * 1.4f);

    #region Traces

    private void UpdateTraces(double deltaSeconds)
    {
        var growSpeed = GrowCellsPerSecond * _cell * SpeedFactor;
        var pulseSpeed = PulseSpeed * Unit * SpeedFactor;
        var glowDecay = (float)deltaSeconds / GlowSeconds;

        for (var i = _traces.Count - 1; i >= 0; i--)
        {
            var trace = _traces[i];

            trace.StartGlow = Math.Max(0f, trace.StartGlow - glowDecay);
            trace.EndGlow = Math.Max(0f, trace.EndGlow - glowDecay);

            switch (trace.State)
            {
                case TraceState.Growing:
                    if (Time >= trace.GrowAfter)
                        Grow(trace, growSpeed * (float)deltaSeconds);
                    break;

                case TraceState.Live:
                    if (!trace.FromEye && Time >= trace.LiveUntil)
                        BeginFade(trace, TraceFadeSeconds);
                    else
                        AdvancePulse(trace, pulseSpeed * (float)deltaSeconds);
                    break;

                case TraceState.Fading:
                    if (Time >= trace.FadeEnd)
                    {
                        Release(trace);
                        _traces.RemoveAt(i);
                    }
                    break;
            }
        }
    }

    private void Grow(Trace trace, float distance)
    {
        trace.Visible += distance;

        while (trace.Visible >= trace.Length)
        {
            if (trace.Cells.Count >= trace.TargetCells || !TryExtend(trace))
            {
                trace.Visible = trace.Length;
                FinishGrowth(trace);
                return;
            }
        }
    }

    /// <summary>
    /// Routes the trace one cell further: mostly straight, 45° turns only after a short straight run, never touching other traces.
    /// </summary>
    private bool TryExtend(Trace trace)
    {
        var head = trace.Cells[trace.Cells.Count - 1];
        var straight = trace.Direction;
        var left = (straight + 7) % 8;
        var right = (straight + 1) % 8;
        var canTurn = trace.Run >= MinRun;

        var straightWeight = CanEnter(head, straight) ? 5.0 : 0;
        var leftWeight = canTurn && CanEnter(head, left) ? 1.0 : 0;
        var rightWeight = canTurn && CanEnter(head, right) ? 1.0 : 0;
        var total = straightWeight + leftWeight + rightWeight;

        if (total <= 0)
            return false;

        var pick = Random.NextDouble() * total;
        var direction = pick < straightWeight ? straight : pick < straightWeight + leftWeight ? left : right;
        var step = _directions[direction];
        var next = new Point(head.X + step.X, head.Y + step.Y);

        Occupy(next, trace.Id);
        trace.Cells.Add(next);
        trace.AddPoint(CellCenter(next));
        trace.Run = direction == trace.Direction ? trace.Run + 1 : 1;
        trace.Direction = direction;

        return true;
    }

    private void FinishGrowth(Trace trace)
    {
        // A stub that could not get anywhere is not worth keeping.
        if (trace.Cells.Count < (trace.FromEye ? MinEyeCells : MinBoardCells))
        {
            BeginFade(trace, 0.6);
            return;
        }

        trace.State = TraceState.Live;
        trace.LiveUntil = Time + TraceLifeMin + Random.NextDouble() * (TraceLifeMax - TraceLifeMin);
        trace.NextPulse = Time + 0.3 + Random.NextDouble() * 1.2;
        trace.Reverse = !trace.FromEye && Random.Next(2) == 0;

        // The end pad flashes as it is "soldered".
        trace.EndGlow = 1f;
    }

    private void AdvancePulse(Trace trace, float distance)
    {
        if (trace.Pulse < 0)
        {
            if (Time < trace.NextPulse)
                return;

            trace.Pulse = 0;

            // Signal leaving the eye: the eye lights up as it sends.
            if (trace.FromEye && !trace.Reverse && _eye != null)
            {
                _eye.Flash = 1f;
                _eye.PadGlow[trace.EyePadIndex] = 1f;
            }

            return;
        }

        trace.Pulse += distance;

        if (trace.Pulse < trace.Length)
            return;

        if (trace.Reverse)
        {
            trace.StartGlow = 1f;

            // Signal arriving at the eye: it lights up as it receives.
            if (trace.FromEye && _eye != null)
            {
                _eye.Flash = 1f;
                _eye.PadGlow[trace.EyePadIndex] = 1f;
            }
        }
        else
        {
            trace.EndGlow = 1f;
        }

        trace.Pulse = -1f;
        trace.NextPulse = Time + (1.2 + Random.NextDouble() * 3.5) / SpeedFactor;
        trace.Reverse = Random.Next(trace.FromEye ? 3 : 2) == 0;
    }

    private void BeginFade(Trace trace, double seconds)
    {
        if (trace.State == TraceState.Fading)
            return;

        trace.State = TraceState.Fading;
        trace.FadeStart = Time;
        trace.FadeEnd = Time + seconds;
        trace.Pulse = -1f;
    }

    private float TraceAlpha(Trace trace)
        => trace.State == TraceState.Fading ? (float)Math.Max(0, (trace.FadeEnd - Time) / Math.Max(0.001, trace.FadeEnd - trace.FadeStart)) : 1f;

    private void SpawnBoardTrace()
    {
        for (var attempt = 0; attempt < 25; attempt++)
        {
            var start = new Point(Random.Next(1, _columns - 1), Random.Next(1, _rows - 1));

            if (!IsClearArea(start) || !IsVisible(CellCenter(start).X, CellCenter(start).Y, -_cell))
                continue;

            var width = Math.Max(1.5f, BoardTraceWidth * Unit);
            var trace = new Trace
            {
                Id = _nextTraceId++,
                Width = width,
                HasStartPad = true,
                StartRing = Random.Next(2) == 0,
                EndRing = Random.Next(2) == 0,
                TargetCells = Random.Next(6, 28),
                Direction = Random.NextDouble() < 0.7 ? Random.Next(4) * 2 : Random.Next(4) * 2 + 1,
                GrowAfter = Time,
            };

            Occupy(start, trace.Id);
            trace.Cells.Add(start);
            trace.AddPoint(CellCenter(start));

            if (!CanEnter(start, trace.Direction))
            {
                Release(trace);
                continue;
            }

            _traces.Add(trace);
            return;
        }
    }

    private int CountBoardTraces()
    {
        var count = 0;

        foreach (var trace in _traces)
        {
            if (!trace.FromEye && trace.State != TraceState.Fading)
                count++;
        }

        return count;
    }

    private int TargetBoardTraces()
    {
        var areaFactor = (double)Width * Height / (1920.0 * 1080.0);

        return Math.Max(2, (int)Math.Round(BoardTracesPer1080p * areaFactor * Settings.Density / SaverSettings.DefaultDensity));
    }

    /// <summary>
    /// Position <paramref name="distance"/> pixels along the trace from its start.
    /// </summary>
    private static PointF PointAt(Trace trace, float distance)
    {
        var points = trace.Points;
        var distances = trace.Distances;

        if (distance <= 0 || points.Count == 1)
            return points[0];

        for (var i = 1; i < points.Count; i++)
        {
            if (distance > distances[i])
                continue;

            var segment = distances[i] - distances[i - 1];
            var t = segment <= 0 ? 1f : (distance - distances[i - 1]) / segment;

            return Lerp(points[i - 1], points[i], t);
        }

        return points[points.Count - 1];
    }

    /// <summary>
    /// The part of the trace drawn so far (all of it once grown).
    /// </summary>
    private static PointF[] VisiblePoints(Trace trace)
    {
        if (trace.State != TraceState.Growing || trace.Visible >= trace.Length)
            return trace.Points.ToArray();

        var result = new List<PointF>();

        for (var i = 0; i < trace.Points.Count; i++)
        {
            if (trace.Distances[i] <= trace.Visible)
            {
                result.Add(trace.Points[i]);
                continue;
            }

            result.Add(PointAt(trace, trace.Visible));
            break;
        }

        return result.ToArray();
    }

    #endregion

    #region Board grid

    private PointF CellCenter(Point cell) => new(_gridOrigin.X + cell.X * _cell, _gridOrigin.Y + cell.Y * _cell);

    private Point NearestCell(PointF point) => new((int)Math.Round((point.X - _gridOrigin.X) / _cell), (int)Math.Round((point.Y - _gridOrigin.Y) / _cell));

    private bool InGrid(int x, int y) => x >= 0 && y >= 0 && x < _columns && y < _rows;

    /// <summary>
    /// Trace id owning the cell, 0 when free, -1 outside the board.
    /// </summary>
    private int Owner(int x, int y) => InGrid(x, y) ? _owners[y * _columns + x] : -1;

    private void Occupy(Point cell, int id)
    {
        if (InGrid(cell.X, cell.Y))
            _owners[cell.Y * _columns + cell.X] = id;
    }

    private void Release(Trace trace)
    {
        foreach (var cell in trace.Cells)
        {
            if (Owner(cell.X, cell.Y) == trace.Id)
                _owners[cell.Y * _columns + cell.X] = 0;
        }

        _eye?.Traces.Remove(trace);
    }

    private bool IsBlocked(int x, int y) => Owner(x, y) != 0 || IsInsideEye(CellCenter(new Point(x, y)));

    /// <summary>
    /// Inside the eye's space or its wordmark block; traces route around both.
    /// </summary>
    private bool IsInsideEye(PointF point)
    {
        if (_eye == null)
            return false;

        if (Distance(point, _eye.Center) < _eye.Radius + _cell * 0.5f)
            return true;

        return _eye.HasBrand && Inflate(_eye.Brand, _cell * 0.6f).Contains(point);
    }

    /// <summary>
    /// A free cell with no trace or eye anywhere around it; a good place for a new trace to start.
    /// </summary>
    private bool IsClearArea(Point cell)
    {
        for (var dy = -1; dy <= 1; dy++)
        {
            for (var dx = -1; dx <= 1; dx++)
            {
                if (IsBlocked(cell.X + dx, cell.Y + dy))
                    return false;
            }
        }

        return true;
    }

    /// <summary>
    /// True when the trace may step from <paramref name="from"/> in <paramref name="direction"/>: the cell is free,
    /// and no trace (itself included) sits right next to it, so traces keep a clean gap and never cross.
    /// </summary>
    private bool CanEnter(Point from, int direction)
    {
        var step = _directions[direction];
        var x = from.X + step.X;
        var y = from.Y + step.Y;

        if (IsBlocked(x, y))
            return false;

        for (var d = 0; d < 8; d += 2)
        {
            var nx = x + _directions[d].X;
            var ny = y + _directions[d].Y;

            if (nx == from.X && ny == from.Y)
                continue;

            if (Owner(nx, ny) > 0)
                return false;
        }

        return true;
    }

    #endregion

    #region Eye

    private void UpdateEye(double deltaSeconds)
    {
        if (_eye == null)
        {
            if (Time >= _nextEyeAt)
                OpenEye();

            return;
        }

        var eye = _eye;

        if (!eye.TracesCreated && Time >= eye.OpenAt + EyeOpenSeconds + 0.2)
            CreateEyeTraces(eye);

        if (!eye.Closing && Time >= eye.CloseAt)
        {
            eye.Closing = true;

            foreach (var trace in eye.Traces)
                BeginFade(trace, EyeCloseSeconds + 1.5);
        }

        if (Time >= eye.CloseAt + EyeCloseSeconds)
        {
            _eye = null;
            _nextEyeAt = Time + EyeGapSeconds;
            return;
        }

        if (!eye.Closing && Time >= eye.NextBlink)
        {
            eye.BlinkStart = Time;
            eye.NextBlink = Time + 3.5 + Random.NextDouble() * 5;
        }

        var decay = (float)deltaSeconds * 2f;

        eye.Flash = Math.Max(0f, eye.Flash - decay);

        for (var i = 0; i < eye.PadGlow.Length; i++)
            eye.PadGlow[i] = Math.Max(0f, eye.PadGlow[i] - decay * 0.6f);

        eye.PupilX += (LookTarget(eye) - eye.PupilX) * (float)Math.Min(1.0, deltaSeconds * 3);
    }

    /// <summary>
    /// Opens the eye somewhere on screen with the wordmark standing beside it, like the logo lockup.
    /// Prefers a spot where few traces are in the way; those fade out quickly.
    /// </summary>
    private void OpenEye()
    {
        var scale = EyeScale * Unit;
        var radius = EyeExtent * scale;
        var gap = radius * BrandGapFactor;
        var margin = _cell;
        var halfHeight = Math.Max(radius, _brandSize.Height / 2);
        var hasBrand = _brandSize.Width > 0
                       && 2 * radius + gap + _brandSize.Width + 2 * margin <= Width
                       && 2 * halfHeight + 2 * margin <= Height;

        var bestCenter = new PointF(Width / 2f, Height / 2f);
        var bestBrand = RectangleF.Empty;
        var bestScore = int.MaxValue;

        for (var attempt = 0; attempt < 16; attempt++)
        {
            PointF center;
            var brand = RectangleF.Empty;

            if (hasBrand)
            {
                var brandLeft = Random.Next(2) == 0;
                var minX = brandLeft ? margin + _brandSize.Width + gap + radius : margin + radius;
                var maxX = brandLeft ? Width - margin - radius : Width - margin - gap - _brandSize.Width - radius;

                center = new PointF(Between(minX, maxX), Between(margin + halfHeight, Height - margin - halfHeight));

                var brandX = brandLeft ? center.X - radius - gap - _brandSize.Width : center.X + radius + gap;

                brand = new RectangleF(brandX, center.Y - _brandSize.Height / 2, _brandSize.Width, _brandSize.Height);
            }
            else
            {
                var marginX = Math.Min(Width / 2f, radius + margin);
                var marginY = Math.Min(Height / 2f, radius + margin);

                center = new PointF(Between(marginX, Width - marginX), Between(marginY, Height - marginY));
            }

            var score = 0;

            foreach (var trace in _traces)
            {
                if (TouchesLockup(trace, center, radius, hasBrand, brand))
                    score++;
            }

            if (score < bestScore)
            {
                bestCenter = center;
                bestBrand = brand;
                bestScore = score;
            }
        }

        foreach (var trace in _traces)
        {
            if (TouchesLockup(trace, bestCenter, radius, hasBrand, bestBrand))
                BeginFade(trace, 0.8);
        }

        _eye = new Eye
        {
            Center = bestCenter,
            Rotation = (float)(Random.NextDouble() * 24 - 12),
            Scale = scale,
            HasBrand = hasBrand,
            Brand = bestBrand,
            OpenAt = Time,
            CloseAt = Time + EyeLifeMin + Random.NextDouble() * (EyeLifeMax - EyeLifeMin),
            NextBlink = Time + 2.5 + Random.NextDouble() * 3,
        };
    }

    private bool TouchesLockup(Trace trace, PointF center, float radius, bool hasBrand, RectangleF brand)
    {
        if (TouchesCircle(trace, center, radius + _cell))
            return true;

        if (!hasBrand)
            return false;

        var area = Inflate(brand, _cell);

        foreach (var point in trace.Points)
        {
            if (area.Contains(point))
                return true;
        }

        return false;
    }

    private float Between(float min, float max) => max <= min ? (min + max) / 2 : min + (float)Random.NextDouble() * (max - min);

    private static RectangleF Inflate(RectangleF rectangle, float amount)
        => new(rectangle.X - amount, rectangle.Y - amount, rectangle.Width + amount * 2, rectangle.Height + amount * 2);

    /// <summary>
    /// One trace out of each of the eye's leg pads, continuing the circuit onto the board.
    /// </summary>
    private void CreateEyeTraces(Eye eye)
    {
        eye.TracesCreated = true;

        for (var p = 0; p < _eyePads.Length; p++)
        {
            var pad = _eyePads[p];
            var padCenter = EyeToWorld(eye, pad.Center);
            var preferred = (pad.Direction + (int)Math.Round(eye.Rotation / 45f)) % 8 + 8;
            var direction = -1;
            Point? start = null;

            // Leave the pad in its natural direction; if the wordmark or another trace is there, bend up to 90° away.
            foreach (var turn in new[] { 0, -1, 1, -2, 2 })
            {
                var candidate = (preferred + turn) % 8;
                var step = _directions[candidate];
                var stepLength = step.X != 0 && step.Y != 0 ? 0.7071f : 1f;

                // First free cell outward from the pad, outside the eye's own space.
                for (var k = 1; k <= 5 && start == null; k++)
                {
                    var probe = new PointF(padCenter.X + step.X * stepLength * _cell * k, padCenter.Y + step.Y * stepLength * _cell * k);
                    var cell = NearestCell(probe);

                    if (!IsBlocked(cell.X, cell.Y) && HasNoNeighbours(cell) && !CrossesBrand(eye, padCenter, CellCenter(cell)))
                        start = cell;
                }

                if (start != null)
                {
                    direction = candidate;
                    break;
                }
            }

            if (start == null)
                continue;

            var trace = new Trace
            {
                Id = _nextTraceId++,
                EyePadIndex = p,
                Width = Math.Max(2f, EyeTraceWidth * Unit),
                EndRing = Random.Next(2) == 0,
                TargetCells = Random.Next(10, 26),
                Direction = direction,
                GrowAfter = Time,
            };

            trace.AddPoint(padCenter);
            Occupy(start.Value, trace.Id);
            trace.Cells.Add(start.Value);
            trace.AddPoint(CellCenter(start.Value));

            _traces.Add(trace);
            eye.Traces.Add(trace);
        }
    }

    /// <summary>
    /// True when the short lead from an eye pad to its first grid cell would cut through the wordmark block.
    /// </summary>
    private bool CrossesBrand(Eye eye, PointF from, PointF to)
    {
        if (!eye.HasBrand)
            return false;

        const int samples = 12;
        var area = Inflate(eye.Brand, _cell * 0.6f);

        for (var i = 0; i <= samples; i++)
        {
            if (area.Contains(Lerp(from, to, (float)i / samples)))
                return true;
        }

        return false;
    }

    private bool HasNoNeighbours(Point cell)
    {
        for (var d = 0; d < 8; d += 2)
        {
            if (Owner(cell.X + _directions[d].X, cell.Y + _directions[d].Y) > 0)
                return false;
        }

        return true;
    }

    /// <summary>
    /// Where the pupil should slide along the lid: toward the nearest signal in flight, otherwise a slow idle drift.
    /// </summary>
    private float LookTarget(Eye eye)
    {
        PointF? target = null;
        var nearest = float.MaxValue;

        foreach (var trace in _traces)
        {
            if (trace.Pulse < 0)
                continue;

            var head = PointAt(trace, trace.Reverse ? trace.Length - trace.Pulse : trace.Pulse);
            var distance = Distance(head, eye.Center) * (trace.FromEye ? 0.5f : 1f);

            if (distance < nearest)
            {
                nearest = distance;
                target = head;
            }
        }

        if (target == null)
            return (float)(PupilRestX * 0.4 + Math.Sin(Time * 0.4 + Phase) * PupilTravel * 0.6);

        // Horizontal offset in the lid's own frame.
        var angle = (eye.Rotation + BarAngle) * Math.PI / 180;
        var dx = target.Value.X - eye.Center.X;
        var dy = target.Value.Y - eye.Center.Y;
        var along = (float)(dx * Math.Cos(angle) + dy * Math.Sin(angle));

        return PupilTravel * Math.Max(-1f, Math.Min(1f, along / (eye.Radius * 1.2f)));
    }

    private float EyePresence(Eye eye)
    {
        var opening = (Time - eye.OpenAt) / EyeOpenSeconds;
        var closing = 1 - (Time - eye.CloseAt) / EyeCloseSeconds;

        return (float)Math.Max(0, Math.Min(1, Math.Min(opening, closing)));
    }

    /// <summary>
    /// 0 = wide open, 1 = shut. Combines opening, closing and blinks.
    /// </summary>
    private float EyeLid(Eye eye)
    {
        var opening = 1 - (Time - eye.OpenAt - 0.2) / (EyeOpenSeconds - 0.2);
        var closing = (Time - eye.CloseAt) / (EyeCloseSeconds - 0.2);
        var blinkPhase = (Time - eye.BlinkStart) / BlinkSeconds;
        var blink = blinkPhase is >= 0 and <= 1 ? 1 - Math.Abs(2 * blinkPhase - 1) : 0;

        return (float)Math.Max(0, Math.Min(1, Math.Max(Math.Max(opening, closing), blink)));
    }

    private static PointF EyeToWorld(Eye eye, PointF local)
    {
        var angle = eye.Rotation * Math.PI / 180;
        var cos = (float)Math.Cos(angle);
        var sin = (float)Math.Sin(angle);
        var x = local.X * eye.Scale;
        var y = local.Y * eye.Scale;

        return new PointF(eye.Center.X + x * cos - y * sin, eye.Center.Y + x * sin + y * cos);
    }

    private static bool TouchesCircle(Trace trace, PointF center, float radius)
    {
        foreach (var point in trace.Points)
        {
            if (Distance(point, center) < radius)
                return true;
        }

        return false;
    }

    #endregion

    #region Drawing

    private void DrawTraces(Graphics graphics)
    {
        foreach (var trace in _traces)
        {
            var alpha = TraceAlpha(trace) * Fade;

            if (alpha <= 0)
                continue;

            var level = trace.FromEye ? EyeTraceLevel : TraceLevel;
            var color = Dim(_grey, Brightness * level, alpha);
            var points = VisiblePoints(trace);

            using var pen = new Pen(color, trace.Width) { StartCap = LineCap.Round, EndCap = LineCap.Round, LineJoin = LineJoin.Round };

            if (points.Length >= 2)
                graphics.DrawLines(pen, points);

            if (trace.HasStartPad)
                DrawPad(graphics, trace.Points[0], trace.StartRing, trace.Width, color, trace.StartGlow * alpha);

            if (trace.State == TraceState.Growing)
            {
                // The router's drill bit: a small yellow tip where the trace is being laid down.
                if (points.Length >= 2)
                {
                    var tip = points[points.Length - 1];
                    var radius = trace.Width * 0.6f;

                    using var tipBrush = new SolidBrush(Dim(_yellow, Accent, 0.9f * alpha));
                    graphics.FillEllipse(tipBrush, tip.X - radius, tip.Y - radius, radius * 2, radius * 2);
                }
            }
            else
            {
                DrawPad(graphics, trace.Points[trace.Points.Count - 1], trace.EndRing, trace.Width, color, trace.EndGlow * alpha);
            }
        }
    }

    /// <summary>
    /// Pad at a trace end: a filled via or a hollow ring, like the eye's legs. Glows yellow with an orange ripple when a signal lands.
    /// </summary>
    private void DrawPad(Graphics graphics, PointF center, bool ring, float traceWidth, Color color, float glow)
    {
        float outer;

        if (ring)
        {
            var stroke = traceWidth * 0.65f;
            var radius = traceWidth * 1.1f;

            outer = radius + stroke / 2;

            using var pen = new Pen(color, stroke);

            graphics.FillEllipse(Brushes.Black, center.X - radius, center.Y - radius, radius * 2, radius * 2);
            graphics.DrawEllipse(pen, center.X - radius, center.Y - radius, radius * 2, radius * 2);
        }
        else
        {
            outer = traceWidth * 1.5f;

            using var brush = new SolidBrush(color);
            graphics.FillEllipse(brush, center.X - outer, center.Y - outer, outer * 2, outer * 2);
        }

        if (glow <= 0)
            return;

        var fill = outer * 0.85f;

        using var glowBrush = new SolidBrush(Dim(_yellow, Accent, glow * Fade));
        graphics.FillEllipse(glowBrush, center.X - fill, center.Y - fill, fill * 2, fill * 2);

        var ripple = outer * (1.2f + (1 - glow) * 2.2f);

        using var ripplePen = new Pen(Dim(_orange, Accent, glow * 0.7f * Fade), Math.Max(1f, traceWidth * 0.35f));
        graphics.DrawEllipse(ripplePen, center.X - ripple, center.Y - ripple, ripple * 2, ripple * 2);
    }

    private void DrawPulses(Graphics graphics)
    {
        const int trailSamples = 8;

        foreach (var trace in _traces)
        {
            if (trace.Pulse < 0)
                continue;

            var length = trace.Length;
            var head = trace.Reverse ? length - trace.Pulse : trace.Pulse;
            var tail = trace.Reverse ? Math.Min(length, head + PulseTrail * Unit) : Math.Max(0, head - PulseTrail * Unit);
            var width = trace.Width * 0.7f;

            using var trailPen = new Pen(Color.Transparent, width) { StartCap = LineCap.Round, EndCap = LineCap.Round };

            var previous = PointAt(trace, tail);

            for (var s = 1; s <= trailSamples; s++)
            {
                var current = PointAt(trace, tail + (head - tail) * s / trailSamples);

                trailPen.Color = Dim(_orange, Accent, (float)s / trailSamples * 0.8f * Fade);
                graphics.DrawLine(trailPen, previous, current);
                previous = current;
            }

            var headPoint = PointAt(trace, head);
            var halo = trace.Width * 1.6f;
            var core = trace.Width * 0.75f;

            using var haloBrush = new SolidBrush(Dim(_yellow, Accent, 0.2f * Fade));
            using var coreBrush = new SolidBrush(Dim(_yellow, Accent, Fade));

            graphics.FillEllipse(haloBrush, headPoint.X - halo, headPoint.Y - halo, halo * 2, halo * 2);
            graphics.FillEllipse(coreBrush, headPoint.X - core, headPoint.Y - core, core * 2, core * 2);
        }
    }

    /// <summary>
    /// Milva's eye, redrawn from the logo as light on black: a half-disc eye hanging from its lid bar,
    /// a sliding pupil, and four legs ending in two vias and two ring pads.
    /// </summary>
    private void DrawEye(Graphics graphics)
    {
        var eye = _eye;

        if (eye == null)
            return;

        var presence = EyePresence(eye) * Fade;

        if (presence <= 0)
            return;

        var lid = EyeLid(eye);
        var color = Dim(_grey, Brightness * EyeLevel, presence);
        var state = graphics.Save();

        try
        {
            graphics.TranslateTransform(eye.Center.X, eye.Center.Y);
            graphics.RotateTransform(eye.Rotation);
            graphics.ScaleTransform(eye.Scale, eye.Scale);

            using var legPen = new Pen(color, LogoStroke) { StartCap = LineCap.Round, EndCap = LineCap.Round, LineJoin = LineJoin.Round };
            using var ringPen = new Pen(color, RingStroke);
            using var bodyBrush = new SolidBrush(color);

            foreach (var leg in _legs)
                graphics.DrawLines(legPen, leg);

            // Eye and pupil live in the lid's frame; a blink squashes them up into the lid bar.
            if (lid < 0.98f)
            {
                var lidState = graphics.Save();

                graphics.RotateTransform(BarAngle);
                graphics.ScaleTransform(1f, 1f - lid);
                graphics.FillPie(bodyBrush, -BodyRadius, -BodyRadius, BodyRadius * 2, BodyRadius * 2, 0f, 180f);
                graphics.FillPie(Brushes.Black, eye.PupilX - PupilRadius, -PupilRadius, PupilRadius * 2, PupilRadius * 2, 0f, 180f);

                if (eye.Flash > 0)
                {
                    using var flashBrush = new SolidBrush(Dim(_yellow, Accent, eye.Flash * presence));
                    graphics.FillPie(flashBrush, eye.PupilX - PupilRadius, -PupilRadius, PupilRadius * 2, PupilRadius * 2, 0f, 180f);
                }

                graphics.Restore(lidState);
            }

            graphics.DrawLine(legPen, _barStart, _barEnd);

            for (var p = 0; p < _eyePads.Length; p++)
            {
                var pad = _eyePads[p];
                var glow = eye.PadGlow[p] * presence;

                if (pad.IsRing)
                {
                    var radius = pad.Radius - RingStroke / 2;

                    graphics.FillEllipse(Brushes.Black, pad.Center.X - radius, pad.Center.Y - radius, radius * 2, radius * 2);
                    graphics.DrawEllipse(ringPen, pad.Center.X - radius, pad.Center.Y - radius, radius * 2, radius * 2);

                    if (glow > 0)
                    {
                        using var glowPen = new Pen(Dim(_yellow, Accent, glow), RingStroke);
                        graphics.DrawEllipse(glowPen, pad.Center.X - radius, pad.Center.Y - radius, radius * 2, radius * 2);
                    }
                }
                else
                {
                    graphics.FillEllipse(bodyBrush, pad.Center.X - pad.Radius, pad.Center.Y - pad.Radius, pad.Radius * 2, pad.Radius * 2);

                    if (glow > 0)
                    {
                        using var glowBrush = new SolidBrush(Dim(_yellow, Accent, glow));
                        graphics.FillEllipse(glowBrush, pad.Center.X - pad.Radius, pad.Center.Y - pad.Radius, pad.Radius * 2, pad.Radius * 2);
                    }
                }
            }
        }
        finally
        {
            graphics.Restore(state);
        }
    }

    /// <summary>
    /// Wordmark and clock beside the eye, on a black plate so nothing shows through. Appears and leaves with the eye.
    /// </summary>
    private void DrawBrand(Graphics graphics)
    {
        var eye = _eye;

        if (eye == null || !eye.HasBrand || _wordmark == null)
            return;

        var presence = EyePresence(eye) * Fade;

        if (presence <= 0)
            return;

        var clock = Settings.ShowClock ? ClockText(DateTime.Now) : null;
        var plate = _cell * 0.6f;
        var state = graphics.Save();

        try
        {
            graphics.TextRenderingHint = TextRenderingHint.AntiAlias;
            graphics.TranslateTransform(eye.Brand.X, eye.Brand.Y);
            graphics.FillRectangle(Brushes.Black, -plate, -plate, eye.Brand.Width + plate * 2, eye.Brand.Height + plate * 2);

            using var letterBrush = new SolidBrush(Dim(_grey, Brightness * WordmarkLevel, presence));
            using var dotBrush = new SolidBrush(Dim(_yellow, Accent, presence));
            using var clockBrush = new SolidBrush(Dim(_grey, Brightness * ClockLevel, presence));

            graphics.FillPath(letterBrush, _wordmark.Letters);
            graphics.FillPath(dotBrush, _wordmark.Dot);

            if (clock != null)
                graphics.DrawString(clock, _clockFont, clockBrush, 0, _wordmark.Size.Height + ClockGap);
        }
        finally
        {
            graphics.Restore(state);
        }
    }

    private float ClockGap => _clockFont.Size * 0.4f;

    private string ClockText(DateTime now) => $"{now.ToString("HH:mm", _culture)}  ·  {now.ToString("d MMMM dddd", _culture)}";

    /// <summary>
    /// Size of the wordmark + clock block, with room for longer dates than today's.
    /// </summary>
    private SizeF MeasureBrand()
    {
        if (!Settings.ShowClock)
            return _wordmark.Size;

        using var bitmap = new Bitmap(1, 1);
        using var graphics = Graphics.FromImage(bitmap);

        graphics.TextRenderingHint = TextRenderingHint.AntiAlias;

        var clock = graphics.MeasureString(ClockText(DateTime.Now), _clockFont);

        return new SizeF(Math.Max(_wordmark.Size.Width, clock.Width * 1.25f), _wordmark.Size.Height + ClockGap + clock.Height);
    }

    #endregion

    private static float Distance(PointF a, PointF b)
    {
        var dx = b.X - a.X;
        var dy = b.Y - a.Y;

        return (float)Math.Sqrt(dx * dx + dy * dy);
    }

    private static PointF Lerp(PointF a, PointF b, float t) => new(a.X + (b.X - a.X) * t, a.Y + (b.Y - a.Y) * t);
}
