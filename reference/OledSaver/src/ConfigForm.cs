using System;
using System.Diagnostics;
using System.Drawing;
using System.Windows.Forms;

namespace OledSaver;

/// <summary>
/// Settings dialog shown for "Ayarlar" in Windows' screen saver settings.
/// </summary>
internal sealed class ConfigForm : Form
{
    private const int TrackBarWidth = 300;

    private readonly SaverSettings _settings;
    private static readonly SaverTheme[] _themes = [SaverTheme.Milvasoft, SaverTheme.Constellation];
    private static readonly string[] _themeNames = ["Milvasoft", "Takımyıldız"];

    private readonly ComboBox _theme = new() { DropDownStyle = ComboBoxStyle.DropDownList, Width = 180, Anchor = AnchorStyles.Left, Margin = new Padding(3, 6, 3, 2) };
    private readonly CheckBox _showClock = new() { Text = "Saati ve tarihi göster", AutoSize = true, Margin = new Padding(3, 8, 3, 8) };
    private readonly TrackBar _brightness = CreateTrackBar(SaverSettings.MinBrightness, SaverSettings.MaxBrightness, 10);
    private readonly TrackBar _density = CreateTrackBar(SaverSettings.MinDensity, SaverSettings.MaxDensity, 20);
    private readonly TrackBar _speed = CreateTrackBar(SaverSettings.MinSpeed, SaverSettings.MaxSpeed, 1);
    private readonly Label _brightnessValue = CreateValueLabel();
    private readonly Label _densityValue = CreateValueLabel();
    private readonly Label _speedValue = CreateValueLabel();

    public ConfigForm(SaverSettings settings)
    {
        _settings = settings;

        SuspendLayout();

        Text = "OledSaver Ayarları";
        Font = SystemFonts.MessageBoxFont;
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = false;
        MinimizeBox = false;
        ShowInTaskbar = true;
        StartPosition = FormStartPosition.CenterScreen;
        AutoSize = true;
        AutoSizeMode = AutoSizeMode.GrowAndShrink;
        Padding = new Padding(12);

        var layout = new TableLayoutPanel
        {
            ColumnCount = 3,
            AutoSize = true,
            AutoSizeMode = AutoSizeMode.GrowAndShrink,
            Dock = DockStyle.Fill,
        };

        var title = new Label
        {
            Text = "OLED ekranlar için hareketli ekran koruyucu",
            AutoSize = true,
            Font = new Font(Font, FontStyle.Bold),
            Margin = new Padding(3, 0, 3, 6),
        };

        var info = new Label
        {
            Text = "Bekleme süresini Windows belirler (kurulumda 5 dk). Video oynarken Windows ekran koruyucuyu zaten başlatmaz.",
            AutoSize = true,
            MaximumSize = new Size(TrackBarWidth + 160, 0),
            ForeColor = SystemColors.GrayText,
            Margin = new Padding(3, 10, 3, 4),
        };

        var windowsSettings = new LinkLabel { Text = "Windows ekran koruyucu ayarlarını aç", AutoSize = true, Margin = new Padding(3, 0, 3, 10) };
        windowsSettings.LinkClicked += (_, _) => OpenWindowsScreenSaverSettings();

        _theme.Items.AddRange(_themeNames);

        AddRow(layout, title);
        AddLabeledRow(layout, "Tema", _theme);
        AddRow(layout, _showClock);
        AddSliderRow(layout, "Parlaklık", _brightness, _brightnessValue);
        AddSliderRow(layout, "Yoğunluk", _density, _densityValue);
        AddSliderRow(layout, "Hız", _speed, _speedValue);
        AddRow(layout, info);
        AddRow(layout, windowsSettings);
        AddRow(layout, CreateButtons());

        Controls.Add(layout);

        _brightness.ValueChanged += (_, _) => UpdateValueLabels();
        _density.ValueChanged += (_, _) => UpdateValueLabels();
        _speed.ValueChanged += (_, _) => UpdateValueLabels();

        ShowValues(settings.Theme, settings.ShowClock, settings.Brightness, settings.Density, settings.Speed);

        AutoScaleDimensions = new SizeF(96F, 96F);
        AutoScaleMode = AutoScaleMode.Dpi;

        ResumeLayout(false);
        PerformLayout();
    }

    private FlowLayoutPanel CreateButtons()
    {
        var buttons = new FlowLayoutPanel
        {
            FlowDirection = FlowDirection.RightToLeft,
            WrapContents = false,
            AutoSize = true,
            AutoSizeMode = AutoSizeMode.GrowAndShrink,
            Dock = DockStyle.Fill,
            Margin = Padding.Empty,
        };

        var cancel = new Button { Text = "İptal", DialogResult = DialogResult.Cancel, AutoSize = true };
        var ok = new Button { Text = "Tamam", AutoSize = true };
        var test = new Button { Text = "Kaydet ve dene", AutoSize = true };
        var defaults = new Button { Text = "Varsayılanlar", AutoSize = true };

        ok.Click += (_, _) =>
        {
            if (SaveChanges())
            {
                DialogResult = DialogResult.OK;
                Close();
            }
        };

        test.Click += (_, _) =>
        {
            if (SaveChanges())
                StartFullScreen();
        };

        defaults.Click += (_, _) => ShowValues(SaverSettings.DefaultTheme, SaverSettings.DefaultShowClock, SaverSettings.DefaultBrightness, SaverSettings.DefaultDensity, SaverSettings.DefaultSpeed);

        buttons.Controls.AddRange([cancel, ok, test, defaults]);

        AcceptButton = ok;
        CancelButton = cancel;

        return buttons;
    }

    private void ShowValues(SaverTheme theme, bool showClock, int brightness, int density, int speed)
    {
        _theme.SelectedIndex = Math.Max(0, Array.IndexOf(_themes, theme));
        _showClock.Checked = showClock;
        _brightness.Value = brightness;
        _density.Value = density;
        _speed.Value = speed;

        UpdateValueLabels();
    }

    private void UpdateValueLabels()
    {
        _brightnessValue.Text = $"%{_brightness.Value}";
        _densityValue.Text = _density.Value.ToString();
        _speedValue.Text = _speed.Value.ToString();
    }

    private bool SaveChanges()
    {
        _settings.Theme = _themes[Math.Max(0, _theme.SelectedIndex)];
        _settings.ShowClock = _showClock.Checked;
        _settings.Brightness = _brightness.Value;
        _settings.Density = _density.Value;
        _settings.Speed = _speed.Value;

        try
        {
            _settings.Save();
            return true;
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, $"Ayarlar kaydedilemedi: {ex.Message}", Text, MessageBoxButtons.OK, MessageBoxIcon.Error);
            return false;
        }
    }

    private void StartFullScreen()
    {
        try
        {
            Process.Start(new ProcessStartInfo(Application.ExecutablePath, "/s") { UseShellExecute = false });
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, $"Başlatılamadı: {ex.Message}", Text, MessageBoxButtons.OK, MessageBoxIcon.Error);
        }
    }

    private void OpenWindowsScreenSaverSettings()
    {
        try
        {
            Process.Start(new ProcessStartInfo("control.exe", "desk.cpl,,@screensaver") { UseShellExecute = true });
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, $"Açılamadı: {ex.Message}", Text, MessageBoxButtons.OK, MessageBoxIcon.Error);
        }
    }

    private static void AddRow(TableLayoutPanel layout, Control control)
    {
        layout.Controls.Add(control, 0, layout.RowCount);
        layout.SetColumnSpan(control, 3);
        layout.RowCount++;
    }

    private static void AddLabeledRow(TableLayoutPanel layout, string caption, Control control)
    {
        var label = new Label { Text = caption, AutoSize = true, Anchor = AnchorStyles.Left, Margin = new Padding(3, 4, 12, 0) };

        layout.Controls.Add(label, 0, layout.RowCount);
        layout.Controls.Add(control, 1, layout.RowCount);
        layout.SetColumnSpan(control, 2);
        layout.RowCount++;
    }

    private static void AddSliderRow(TableLayoutPanel layout, string caption, TrackBar trackBar, Label value)
    {
        var label = new Label { Text = caption, AutoSize = true, Anchor = AnchorStyles.Left, Margin = new Padding(3, 0, 12, 0) };

        layout.Controls.Add(label, 0, layout.RowCount);
        layout.Controls.Add(trackBar, 1, layout.RowCount);
        layout.Controls.Add(value, 2, layout.RowCount);
        layout.RowCount++;
    }

    private static TrackBar CreateTrackBar(int minimum, int maximum, int tickFrequency) => new()
    {
        Minimum = minimum,
        Maximum = maximum,
        TickFrequency = tickFrequency,
        SmallChange = 1,
        LargeChange = tickFrequency,
        Width = TrackBarWidth,
        Anchor = AnchorStyles.Left | AnchorStyles.Right,
    };

    private static Label CreateValueLabel() => new()
    {
        AutoSize = true,
        MinimumSize = new Size(40, 0),
        Anchor = AnchorStyles.Left,
        TextAlign = ContentAlignment.MiddleLeft,
    };
}
