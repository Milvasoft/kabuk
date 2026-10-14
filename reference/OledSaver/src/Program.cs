using System;
using System.Threading;
using System.Windows.Forms;

namespace OledSaver;

/// <summary>
/// Screen saver entry point. Windows starts .scr files with:
/// <list type="bullet">
/// <item><c>/s</c> run full screen,</item>
/// <item><c>/p HWND</c> draw a preview inside the given window,</item>
/// <item><c>/c[:HWND]</c> or no argument: show settings.</item>
/// </list>
/// </summary>
internal static class Program
{
    private const string RunningMutexName = @"Local\OledSaver.Running";

    private enum Mode
    {
        Configure,
        FullScreen,
        Preview,
    }

    [STAThread]
    private static int Main(string[] args)
    {
        var (mode, handle) = ParseArguments(args);

        return mode switch
        {
            Mode.FullScreen => RunFullScreen(),
            Mode.Preview => RunPreview(handle),
            _ => RunConfigure(handle),
        };
    }

    private static int RunFullScreen()
    {
        NativeMethods.EnablePerMonitorDpiAwareness();

        // Windows can fire the saver again while one is already up (e.g. "Kaydet ve dene" racing the idle timer).
        using var mutex = new Mutex(true, RunningMutexName, out var createdNew);

        if (!createdNew)
            return 0;

        StartWinForms();
        Application.Run(new SaverHost(SaverSettings.Load()));

        return 0;
    }

    private static int RunPreview(IntPtr parent)
    {
        if (parent == IntPtr.Zero || !NativeMethods.WindowExists(parent))
            return 0;

        NativeMethods.EnablePerMonitorDpiAwareness();
        StartWinForms();
        Application.Run(new SaverHost(SaverSettings.Load(), parent));

        return 0;
    }

    private static int RunConfigure(IntPtr owner)
    {
        NativeMethods.EnableSystemDpiAwareness();
        StartWinForms();

        using var form = new ConfigForm(SaverSettings.Load());

        if (owner != IntPtr.Zero && NativeMethods.WindowExists(owner))
            form.ShowDialog(new Win32Window(owner));
        else
            form.ShowDialog();

        return 0;
    }

    private static void StartWinForms()
    {
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
    }

    /// <summary>
    /// Accepts every form Windows and users use: "/s", "-S", "/p 1234", "/p:1234", "/c:1234".
    /// </summary>
    private static (Mode Mode, IntPtr Handle) ParseArguments(string[] args)
    {
        if (args.Length == 0)
            return (Mode.Configure, IntPtr.Zero);

        var flag = args[0].Trim().TrimStart('/', '-');
        string handleText = null;
        var colon = flag.IndexOf(':');

        if (colon >= 0)
        {
            handleText = flag.Substring(colon + 1);
            flag = flag.Substring(0, colon);
        }
        else if (args.Length > 1)
        {
            handleText = args[1];
        }

        var handle = long.TryParse(handleText, out var value) ? new IntPtr(value) : IntPtr.Zero;

        return flag.ToLowerInvariant() switch
        {
            "s" => (Mode.FullScreen, IntPtr.Zero),
            "p" or "l" => (Mode.Preview, handle),
            _ => (Mode.Configure, handle),
        };
    }
}
