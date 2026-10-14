using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Windows.Forms;

namespace OledSaver;

/// <summary>
/// Owns the saver windows and drives them from a single frame timer.
/// </summary>
internal sealed class SaverHost : ApplicationContext
{
    private const int FrameIntervalMilliseconds = 33;
    private const double MaxFrameSeconds = 0.1;
    private const int MouseMoveThreshold = 10;

    private readonly List<SaverForm> _forms = [];
    private readonly Timer _timer = new() { Interval = FrameIntervalMilliseconds };
    private readonly Stopwatch _stopwatch = Stopwatch.StartNew();
    private readonly IntPtr _previewParent;
    private readonly Point _startCursor;
    private double _lastSeconds;
    private bool _exiting;

    /// <summary>
    /// Full screen mode: one window on every connected monitor.
    /// </summary>
    public SaverHost(SaverSettings settings)
    {
        _startCursor = Cursor.Position;

        var screens = Screen.AllScreens;

        for (var i = 0; i < screens.Length; i++)
            _forms.Add(new SaverForm(Scene.Create(settings, i), this, screens[i].Bounds));

        foreach (var form in _forms)
            form.Show();

        _forms[0].Activate();
        Cursor.Hide();

        Start();
    }

    /// <summary>
    /// Preview mode: a single window inside the settings dialog's little monitor.
    /// </summary>
    public SaverHost(SaverSettings settings, IntPtr previewParent)
    {
        _previewParent = previewParent;

        var form = new SaverForm(Scene.Create(settings, 0), this, previewParent);

        _forms.Add(form);
        form.Show();

        Start();
    }

    /// <summary>
    /// Closes the saver once the mouse has really moved, ignoring sensor jitter.
    /// </summary>
    public void OnMouseMoved()
    {
        var position = Cursor.Position;

        if (Math.Abs(position.X - _startCursor.X) > MouseMoveThreshold || Math.Abs(position.Y - _startCursor.Y) > MouseMoveThreshold)
            Exit();
    }

    /// <summary>
    /// Closes every window and ends the message loop.
    /// </summary>
    public void Exit()
    {
        if (_exiting)
            return;

        _exiting = true;
        _timer.Stop();

        foreach (var form in _forms)
            form.Close();

        ExitThread();
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _timer.Dispose();

            foreach (var form in _forms)
                form.Dispose();
        }

        base.Dispose(disposing);
    }

    private void Start()
    {
        _timer.Tick += OnTick;
        _timer.Start();
    }

    private void OnTick(object sender, EventArgs e)
    {
        // The settings dialog destroys its preview host without telling us; leave with it.
        if (_previewParent != IntPtr.Zero && !NativeMethods.WindowExists(_previewParent))
        {
            Exit();
            return;
        }

        var now = _stopwatch.Elapsed.TotalSeconds;
        var delta = Math.Min(MaxFrameSeconds, now - _lastSeconds);

        _lastSeconds = now;

        foreach (var form in _forms)
            form.Advance(delta);
    }
}
