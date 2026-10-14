using System;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace OledSaver;

/// <summary>
/// Win32 interop used by the screen saver.
/// </summary>
internal static class NativeMethods
{
    public const int GWL_STYLE = -16;
    public const int WS_CHILD = 0x40000000;
    public const int WM_DPICHANGED = 0x02E0;

    private static readonly IntPtr _perMonitorAwareV2 = new(-4);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll")]
    public static extern IntPtr SetParent(IntPtr hWndChild, IntPtr hWndNewParent);

    [DllImport("user32.dll")]
    public static extern int GetWindowLong(IntPtr hWnd, int nIndex);

    [DllImport("user32.dll")]
    public static extern int SetWindowLong(IntPtr hWnd, int nIndex, int dwNewLong);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool IsWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool GetClientRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetProcessDpiAwarenessContext(IntPtr value);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetProcessDPIAware();

    /// <summary>
    /// Makes screen bounds come back in physical pixels on every monitor, so full screen windows cover scaled displays exactly.
    /// </summary>
    public static void EnablePerMonitorDpiAwareness()
    {
        try
        {
            if (SetProcessDpiAwarenessContext(_perMonitorAwareV2))
                return;
        }
        catch (Exception ex) when (ex is EntryPointNotFoundException or DllNotFoundException)
        {
            // Older Windows or non-Windows runtime; fall back below.
        }

        EnableSystemDpiAwareness();
    }

    /// <summary>
    /// System DPI awareness; lets WinForms auto scale the settings dialog.
    /// </summary>
    public static void EnableSystemDpiAwareness()
    {
        try
        {
            SetProcessDPIAware();
        }
        catch (Exception ex) when (ex is EntryPointNotFoundException or DllNotFoundException)
        {
            // Nothing to do; window simply stays DPI unaware.
        }
    }

    /// <summary>
    /// Returns true when <paramref name="handle"/> points to an existing window. Always true off Windows.
    /// </summary>
    public static bool WindowExists(IntPtr handle)
    {
        try
        {
            return IsWindow(handle);
        }
        catch (Exception ex) when (ex is EntryPointNotFoundException or DllNotFoundException)
        {
            return true;
        }
    }
}

/// <summary>
/// Wraps a raw window handle so it can own a WinForms dialog.
/// </summary>
internal sealed class Win32Window(IntPtr handle) : IWin32Window
{
    public IntPtr Handle { get; } = handle;
}
