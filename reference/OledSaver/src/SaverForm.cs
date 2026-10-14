using System;
using System.Drawing;
using System.Windows.Forms;

namespace OledSaver;

/// <summary>
/// One borderless window per monitor, or the small preview inside Windows' screen saver settings.
/// </summary>
internal sealed class SaverForm : Form
{
    private readonly Scene _scene;
    private readonly SaverHost _host;
    private readonly IntPtr _previewParent;
    private readonly Rectangle _screenBounds;

    /// <summary>
    /// Full screen window covering <paramref name="screenBounds"/>.
    /// </summary>
    public SaverForm(Scene scene, SaverHost host, Rectangle screenBounds) : this(scene, host)
    {
        _screenBounds = screenBounds;

        TopMost = true;
        Bounds = screenBounds;
    }

    /// <summary>
    /// Preview window hosted inside <paramref name="previewParent"/>.
    /// </summary>
    public SaverForm(Scene scene, SaverHost host, IntPtr previewParent) : this(scene, host)
    {
        _previewParent = previewParent;
    }

    private SaverForm(Scene scene, SaverHost host)
    {
        _scene = scene;
        _host = host;

        Text = "OledSaver";
        FormBorderStyle = FormBorderStyle.None;
        StartPosition = FormStartPosition.Manual;
        ShowInTaskbar = false;
        BackColor = Color.Black;

        SetStyle(ControlStyles.AllPaintingInWmPaint | ControlStyles.UserPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.Opaque, true);
    }

    /// <summary>
    /// True for the settings dialog preview; such a window ignores input.
    /// </summary>
    public bool IsPreview => _previewParent != IntPtr.Zero;

    /// <summary>
    /// Advances the scene and schedules a repaint.
    /// </summary>
    public void Advance(double deltaSeconds)
    {
        _scene.Update(deltaSeconds);
        Invalidate();
    }

    protected override void OnLoad(EventArgs e)
    {
        base.OnLoad(e);

        if (IsPreview)
        {
            NativeMethods.SetParent(Handle, _previewParent);
            NativeMethods.SetWindowLong(Handle, NativeMethods.GWL_STYLE, NativeMethods.GetWindowLong(Handle, NativeMethods.GWL_STYLE) | NativeMethods.WS_CHILD);
            NativeMethods.GetClientRect(_previewParent, out var rect);

            Location = Point.Empty;
            Size = new Size(rect.Right - rect.Left, rect.Bottom - rect.Top);
        }
        else
        {
            // Re-apply in case the window was moved or scaled while it was being created on another monitor.
            Bounds = _screenBounds;
        }

        _scene.Resize(ClientSize.Width, ClientSize.Height);
    }

    protected override void WndProc(ref Message m)
    {
        // The window must keep matching its monitor's physical bounds; swallow DPI change rescaling.
        if (m.Msg == NativeMethods.WM_DPICHANGED)
        {
            m.Result = IntPtr.Zero;
            return;
        }

        base.WndProc(ref m);
    }

    protected override void OnSizeChanged(EventArgs e)
    {
        base.OnSizeChanged(e);
        _scene.Resize(ClientSize.Width, ClientSize.Height);
    }

    protected override void OnPaintBackground(PaintEventArgs e)
    {
        // Scene clears the whole surface itself; skipping this avoids flicker.
    }

    protected override void OnPaint(PaintEventArgs e) => _scene.Draw(e.Graphics);

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);

        if (!IsPreview)
            _host.OnMouseMoved();
    }

    protected override void OnMouseDown(MouseEventArgs e)
    {
        base.OnMouseDown(e);
        ExitOnInput();
    }

    protected override void OnMouseWheel(MouseEventArgs e)
    {
        base.OnMouseWheel(e);
        ExitOnInput();
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        ExitOnInput();
    }

    protected override bool ProcessCmdKey(ref Message msg, Keys keyData)
    {
        // Arrow keys, Tab, Alt combinations etc. never reach OnKeyDown on a form; catch them here.
        if (!IsPreview)
        {
            _host.Exit();
            return true;
        }

        return base.ProcessCmdKey(ref msg, keyData);
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
            _scene.Dispose();

        base.Dispose(disposing);
    }

    private void ExitOnInput()
    {
        if (!IsPreview)
            _host.Exit();
    }
}
