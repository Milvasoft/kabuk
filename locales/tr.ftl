## Kabuk — Türkçe

## Command line

cli-usage =
    Kullanım:
      kabuk                   Kabuk'u başlatır
      kabuk --restore         Acil geri alma: Kabuk'u durdurur, gizlenen pencereleri ve görev çubuğunu geri getirir
      kabuk --install         Kabuk'u senin için kurar (yönetici izni gerekmez)
      kabuk --uninstall       Her şeyi geri alır ve Kabuk'u kaldırır; ayarların kalır
      kabuk --startup on|off  Kabuk oturum açınca başlasın ya da başlamasın
      kabuk --version         Bu Kabuk'un sürümünü yazar
      kabuk msg <komut>       Çalışan Kabuk'a komut gönderir:
                                workspace <1-9|next|prev>, move-to <1-9>,
                                focus <left|down|up|right>, move <left|down|up|right>,
                                float <toggle|on|off>, close, scratchpad <toggle|send>,
                                group <toggle|next>, layout <dwindle|master|floating|tiling|cycle>,
                                dropdown, desktop, launcher, overview, apps, keys, saver,
                                panel <quick|network|calendar|volume>, profile <ad>, reload,
                                query <state|windows|monitors|tray>, quiet <on|off|toggle>, restore
    Seçenekler:
      --state-dir <klasör>    Durum dosyası ve log klasörü
      --config <dosya>        Yapılandırma dosyası
cli-version = Kabuk { $value }
cli-error-unknown = Bilinmeyen argüman: { $arg }
cli-error-missing-value = { $flag } bir değer bekliyor.
cli-error-bad-value = { $flag } için geçersiz değer: { $value }
cli-error-not-with-restore = { $flag }, --restore ile birlikte kullanılamaz.
cli-error-not-with-msg = { $flag }, msg ile birlikte kullanılamaz.
cli-error-not-with-setup = { $flag }, --install, --uninstall ya da --startup ile birlikte kullanılamaz.
cli-msg-empty = Bir komut gerekiyor.
cli-msg-unknown = Bilinmeyen komut: { $command }
cli-msg-bad-args = { $command } için argümanlar yanlış.
cli-msg-not-running = Kabuk çalışmıyor.
cli-msg-unreadable = Kabuk'un yanıtı okunamadı.
ipc-error-no-wm = Pencere yöneticisi kapalı.
ipc-error-no-bar = Bar kapalı.
ipc-error-bad-request = Komut anlaşılmadı.
cli-error-debug-unavailable = { $flag } yalnızca test sürümünde kullanılabilir.
cli-restore-done =
    { $windows ->
        [0] Geri alındı. Gizli pencere yoktu.
       *[other] Geri alındı: { $windows } pencere gösterildi.
    }
cli-restore-state-unreadable = Durum dosyası okunamadı ({ $path }). Görev çubuğu yine de gösterildi; gizli pencereler bulunamadı.

## Startup

startup-failed-title = Kabuk başlatılamadı
startup-failed-state = Durum dosyası yazılamıyor: { $path }. Kabuk hiçbir pencereyi gizlemeden kapandı.

## Watchdog

watchdog-gave-up-title = Kabuk durdu
watchdog-gave-up-body =
    Kabuk { $seconds } saniye içinde { $count } kez çöktü ve yeniden başlatılmayacak. Gizlenen pencereler ve görev çubuğu geri getirildi.

    Ayrıntılar: { $log }

    Kabuk'u Başlat menüsünden (Kabuk) yeniden başlatabilirsin.

## Dates, times and numbers
## Day and month names come from locales/design (the design page).

date-short = { $weekday } { $day } { $month }
decimal-separator = {","}

## Bar

bar-net-rate = { $value } MB/s
bar-percent = { $value }%
# The battery's time left in the quick settings panel (DECISIONS D88).
qs-hours-minutes = { $hours } sa { $minutes } dk
bar-ram = { $value }G
bar-temp = { $value }°
bar-tray-more = +{ $value }

## Konsol names its first workspaces like tmux windows (from the design page's CSS).

konsol-ws-1 = :code
konsol-ws-2 = :web
konsol-ws-3 = :term
konsol-ws-4 = :docs
konsol-ws-5 = :chat
konsol-ws-6 = :misc
## A workspace the user named, in Konsol's way (PLAN M19 U20).
konsol-ws-named = :{ $name }

## Launcher

launcher-kind-window = Pencere
launcher-window-where = { $workspace }, Monitör { $monitor }
launcher-restart = Yeniden başlat
launcher-shutdown = Bilgisayarı kapat
launcher-settings = Kabuk ayarları
power-sign-out = Oturumu kapat
power-sleep = Uyku
vol-mixer = Ses karıştırıcısını aç
vol-apps = Ses düzeyi karıştırıcı
vol-settings = Ses ayarları
vol-troubleshoot = Ses sorunlarını gider
clock-adjust = Tarih ve saati ayarla
notif-focus = Rahatsız etme
notif-quiet-on = Kabuk'un bildirimlerini beklet
notif-quiet-off = Kabuk'un bildirimlerini yeniden göster
stats-task-manager = Görev Yöneticisi
stats-resource-monitor = Kaynak İzleyicisi
net-troubleshoot = Ağ sorunlarını gider
app-pin = Bara sabitle
app-unpin = Bardan kaldır
app-float-rule = Hep yüzer aç
app-float-rule-off = Yüzer açmayı bırak
app-ws-rule = Hep { $value }. çalışma alanında aç
app-ws-rule-off = { $value }. çalışma alanında açmayı bırak
app-close = Pencereyi kapat
app-close-all = Tüm pencereleri kapat
app-recent = Son kullanılanlar
app-pinned = Sabitlenmiş
app-frequent = Sık kullanılanlar
app-tasks = Görevler

## Tüm uygulamalar (macOS'un Launchpad'i gibi sayfa sayfa her uygulama)

grid-search = Ara
grid-empty = Eşleşen uygulama yok
grid-done = Bitti
lp-grid = Tüm uygulamalar
lp-grid-title = Tüm uygulamalar
media-nothing = Çalan bir şey yok
k-profile = Düzen profili
k-profile-open = { $value } düzenini aç
pf = Düzen profilleri
pf-h = Programların monitörlere ve çalışma alanlarına dağılımı; şimdi açık pencerelerden kaydedilir, tek seferde yeniden açılır (başlatıcıdan da)
pf-count = { $value } program
pf-open = Aç
pf-none = Henüz düzen profili yok
pf-name = Profil adı
pf-name-ph = Kodlama, Toplantı…
pf-save = Şimdiki düzeni kaydet
pf-name-missing = Önce profile bir ad ver
pf-empty = Kaydedilecek pencere yok: Kabuk çalışmıyor ya da açık program yok
pf-not-running = Kabuk çalışmıyor
pf-opened = Açılıyor; açık olmayan programlar başlayıp yerlerini alacak
th = Tema dosyası
th-h = Teman, renklerin, barın, imleçlerin ve galeri duvar kâğıdın paylaşmak için tek dosyada; bir tema dosyası kısayollarını, kurallarını ya da programlarını asla değiştirmez
th-export = Dışa aktar…
th-import = İçe aktar…
th-files = Kabuk teması
th-exported = Kaydedildi: { $value }
th-imported = Tema uygulandı: { $value }
th-failed = Bu dosya, bu Kabuk'un okuyabildiği bir Kabuk teması değil
th-unreadable = Tema dosyası okunamadı: { $value }
th-not-saved = Tema dosyası kaydedilemedi: { $value }
set-file-not-written = kabuk.toml yazılamadı: { $value }
set-io-missing = dosya orada değil
set-io-denied = Windows, Kabuk'un onu açmasına izin vermiyor
set-io-not-text = bir metin dosyası değil
dd = Açılır terminal
dd-h = Kısayoluyla ekranın üstünden iner, tekrar basınca saklanır; tüm çalışma alanlarının üstünde durur
dd-command = Program
dd-command-h = Argümanlarıyla; boşsa Windows Terminal, yoksa PowerShell
dd-command-ph = Windows Terminal
dd-height = Yükseklik
set-key-dropdown = Açılır terminal
hc = Sıcak köşeler
hc-h = Fareyi ekranın bir köşesine iterek bir şey yap; iki monitörün birleştiği köşe bir şey yapmaz
hc-top-left = Sol üst
hc-top-right = Sağ üst
hc-bottom-left = Sol alt
hc-bottom-right = Sağ alt
hc-none = Hiçbir şey
hc-app-grid = Tüm uygulamalar
hc-overview = Tüm çalışma alanları (genel bakış)
hc-launcher = Başlatıcı
hc-saver = Ekran koruyucu
hc-saver-h = Ekran koruyucuyu hemen başlatır, seçili koruyucu yoksa ekranları kapatır; yalnızca Kabuk çalışırken
set-saver-corner = Köşeden başlat
set-saver-corner-on = Fareyi { $value } köşeye ittiğinde hemen başlar; yalnızca Kabuk çalışırken
set-saver-corner-off = Hiçbir köşe onu başlatmıyor. Burada seçilen köşe, fare oraya itildiğinde onu başlatır; yalnızca Kabuk çalışırken
gs = Dokunmatik yüzey hareketleri
gs-h = Hassas dokunmatik yüzeyde üç ya da dört parmakla kaydırın; önce Windows'un aynı sayıda parmakla yaptığı hareketleri "Hiçbir şey" yapın, yoksa ikisi birden olur
gs-on = Kaydırma hareketleri
gs-on-h = Varsayılan olarak kapalı
gs-fingers = Parmak
gs-left = Sola kaydır
gs-right = Sağa kaydır
gs-up = Yukarı kaydır
gs-down = Aşağı kaydır
gs-next = Sonraki çalışma alanı
gs-previous = Önceki çalışma alanı
gs-windows = Windows dokunmatik yüzey ayarları
lp-grid-h = − ile gizlediğin uygulamalar; uygulamaları taşımak ya da gizlemek için ızgarada birine basılı tut ya da sağ tıkla
lp-grid-none = Gizlenmiş uygulama yok
lp-grid-show = Göster
lp-grid-reset = Alfabetik sıraya dön
grid-edit-h = Taşımak için sürükle · − gizler · Ayarlar › Başlatıcı geri getirir

## Galeri (DECISIONS D58): her tasarımın adı ve sözleri

gal-walls-live = Galeri · canlı duvar kâğıtları
gal-walls-still = Galeri · sabit duvar kâğıtları
gal-savers-live = Galeri · canlı ekran koruyucular
gal-savers-still = Galeri · sabit ekran koruyucular
gal-still-saver-h = Yavaşça kayan ve nefes alan sabit bir resim; hiçbir şey ekrana yapışmaz
gal-walls-as-savers = Galeri · ekran koruyucu olarak canlı duvar kâğıtları
gal-wall-saver-h = Ekran koruyuculardan parlak: OLED ekranlar için değil
gal-wall-live-aurora-shore = Kuzey ışıkları kıyısı
gal-wall-live-aurora-shore-h = Durgun gölün üstünde kuzey ışıkları
gal-wall-live-contour-breath = Nefes alan izohipsler
gal-wall-live-contour-breath-h = Tepeleri yavaşça nefes alan koyu bir harita
gal-wall-live-ink-bloom = Mürekkep çiçeği
gal-wall-live-ink-bloom-h = Durgun suda açılan çivit mürekkep
gal-wall-live-leaf-shallows = Yapraklı sığlık
gal-wall-live-leaf-shallows-h = Deniz tabanında örülen güneş ışığı
gal-wall-live-milva-desk = Milva devre kartı
gal-wall-live-milva-desk-h = Sakin bir kartta Milvasoft gözü
gal-wall-live-milva-desk-2 = Milva kabartma
gal-wall-live-milva-desk-2-h = Grafitte Milvasoft gözü, çevresinde sıcak bir ışık
gal-wall-live-milva-desk-3 = Milva takımyıldızı
gal-wall-live-milva-desk-3-h = Gece göğünde altın yıldızlarla çizilmiş Milvasoft gözü
gal-wall-live-milva-desk-4 = Milva sıvı altın
gal-wall-live-milva-desk-4-h = Erimiş altından Milvasoft gözü, çevresinde yayılan mürekkep halkaları
gal-wall-live-milva-desk-5 = Milva fener
gal-wall-live-milva-desk-5-h = Gece denizinde deniz feneri olmuş Milvasoft gözü
gal-wall-live-milva-desk-6 = Milva Bauhaus
gal-wall-live-milva-desk-6-h = Sarı ve grafitten düz bir afişte Milvasoft gözü
gal-wall-live-paper-dunes = Kâğıt kum tepeleri
gal-wall-live-paper-dunes-h = Kayan güneşin altında kesik kâğıt tepeler
gal-wall-live-quiet-orrery = Sessiz gezegen düzeneği
gal-wall-live-quiet-orrery-h = Eğik yörüngelerde sade gezegenler
gal-wall-live-rain-glass-city = Camdaki yağmur
gal-wall-live-rain-glass-city-h = Yağmur damlalarının ardında bulanık gece şehri
gal-wall-live-seigaiha-tide = Seigaiha dalgası
gal-wall-live-seigaiha-tide-h = Çivit renginde dalgalanan Japon dalga pulları
gal-wall-live-silk-loom = İpek dokuma
gal-wall-live-silk-loom-h = Altından üstünden geçen saten şeritler
gal-wall-live-velvet-mesh = Kadife renk örgüsü
gal-wall-live-velvet-mesh-h = Birbirine karışan yumuşak renk havuzları
gal-wall-live-iznik-glaze = İznik sırı
gal-wall-live-iznik-glaze-h = Sır ışığı altında yavaşça dönen Osmanlı çinileri
gal-wall-live-ebru-flow = Akan ebru
gal-wall-live-ebru-flow-h = Dalga dalga taranmış, yavaşça akan ebru boyaları
gal-wall-live-chrome-drift = Krom damlalar
gal-wall-live-chrome-drift-h = Birleşip ayrılan sıvı metal damlaları
gal-wall-live-riso-drift = Riso baskı
gal-wall-live-riso-drift-h = Güneş ve tepelerden iki mürekkepli raster baskı, yavaşça kayıyor
gal-wall-live-paper-pleats = Kıvrımlı kâğıt
gal-wall-live-paper-pleats-h = Yumuşak bir lambanın altında nefes alan kavisli kâğıt kıvrımları
gal-wall-live-pine-mist = Sisli çam sırtları
gal-wall-live-pine-mist-h = Şafak sisine karışan, sisi yavaşça süzülen çam sırtları
gal-wall-live-cloud-sea = Bulut denizi
gal-wall-live-cloud-sea-h = Zirveden görülen, şafakta dalgalanan bulutlar
gal-wall-live-obsidian-seams = Obsidyen damarları
gal-wall-live-obsidian-seams-h = Damarlarında yavaşça kızıllaşan volkanik cam
gal-wall-live-wheat-wind = Buğdaydaki rüzgâr
gal-wall-live-wheat-wind-h = Altın renkli buğday tarlasında dolaşan ağır rüzgâr
gal-wall-live-desert-night = Çöl gecesi
gal-wall-live-desert-night-h = Yavaşça dönen Samanyolu'nun altında ay ışığında kum tepeleri
gal-wall-still-agate-slice = Akik dilimi
gal-wall-still-agate-slice-h = Arduvaz üstünde cilalı mavi akik
gal-wall-still-bauhaus-afternoon = Bauhaus öğleden sonrası
gal-wall-still-bauhaus-afternoon-h = Krem kâğıda basılı bir Bauhaus afişi
gal-wall-still-clay-cairn = Kil taş kulesi
gal-wall-still-clay-cairn-h = Dengede duran pastel kil taşlar
gal-wall-still-ginkgo-lines = Altın ginkgo
gal-wall-still-ginkgo-lines-h = Orman yeşili üstünde altın çizgiler
gal-wall-still-isometric-isle = İzometrik ada
gal-wall-still-isometric-isle-h = Süzülen küçük bir küp dünya adası
gal-wall-still-light-ribbon = Işık kurdeleleri
gal-wall-still-light-ribbon-h = Yarı saydam mor ışık kurdeleleri
gal-wall-still-low-poly-dusk = Low poly alacakaranlık
gal-wall-still-low-poly-dusk-h = Gün batımında üçgenlere bölünmüş dağlar
gal-wall-still-paper-hills-morning = Kâğıt tepeler
gal-wall-still-paper-hills-morning-h = Kesik kâğıttan sisli bir sabah vadisi
gal-wall-still-riso-ringed-planet = Riso gezegen
gal-wall-still-riso-ringed-planet-h = İki renkli riso baskı halkalı gezegen
gal-wall-still-rose-silk-folds = Gül ipeği
gal-wall-still-rose-silk-folds-h = Derin kıvrımlarla gül ve şarap rengi ipek
gal-saver-live-abyssal-jellies = Derinlerin denizanaları
gal-saver-live-abyssal-jellies-h = Derinlikte yükselen ışıldayan denizanaları
gal-saver-live-comet-calligraphy = Kuyruklu yıldız hattı
gal-saver-live-comet-calligraphy-h = Ağır ağır bitişik yazan bir kuyruklu yıldız
gal-saver-live-firefly-meadow = Çayır üstünde ateşböcekleri
gal-saver-live-firefly-meadow-h = Karanlık tepelerde yanıp sönen ateşböcekleri
gal-saver-live-frost-bloom = Kırağı çiçeği
gal-saver-live-frost-bloom-h = Büyüyüp eriyen kar kristalleri
gal-saver-live-light-petals = Düşen ışık yaprakları
gal-saver-live-light-petals-h = Yumuşak pembe ışıktan kiraz yaprakları
gal-saver-live-neon-rosettes = Neon güller
gal-saver-live-neon-rosettes-h = Gül eğrileri çizen neon bir kalem
gal-saver-live-night-rain-pond = Gölette gece yağmuru
gal-saver-live-night-rain-pond-h = Ay ışıklı gölette halkalar açan damlalar
gal-saver-live-quiet-orrery = Sessiz gezegen saati
gal-saver-live-quiet-orrery-h = Nefes alan güneşin çevresinde dönen gezegenler
gal-saver-live-smoke-rings = Işıktan duman halkaları
gal-saver-live-smoke-rings-h = Yükselip saçaklanan ışıklı halkalar
gal-saver-live-wireframe-galaxy = Tel kafes galaksi
gal-saver-live-wireframe-galaxy-h = Yavaşça dönen tel kafes sarmal galaksi
gal-saver-still-celestial-chart = Gök haritası
gal-saver-still-celestial-chart-h = Işıkla çizilmiş eski bir yıldız haritası
gal-saver-still-dandelion-clock = Karahindiba
gal-saver-still-dandelion-clock-h = Tohumları uçuşan bir karahindiba
gal-saver-still-deep-sea-jellyfish = Derin deniz denizanası
gal-saver-still-deep-sea-jellyfish-h = İçten aydınlanan cam gibi bir denizanası
gal-saver-still-ember-enso = Kor ensō
gal-saver-still-ember-enso-h = Tek bir kızıl fırça çemberi
gal-saver-still-kintsugi-bowl = Kintsugi kâse
gal-saver-still-kintsugi-bowl-h = Altınla onarılmış siyah bir kâse
gal-saver-still-light-painted-spiral = Işıkla çizilmiş sarmal
gal-saver-still-light-painted-spiral-h = Sarmal çizen renkli ışıklar
gal-saver-still-moon-on-still-water = Durgun suda ay
gal-saver-still-moon-on-still-water-h = Sakin gümüş denizin üstünde hilal
gal-saver-still-neon-arcade = Neon kemerler
gal-saver-still-neon-arcade-h = Islak zeminde yansıyan neon kemerler
gal-saver-still-ringed-planet-nightside = Halkalı gezegen, gece yüzü
gal-saver-still-ringed-planet-nightside-h = Arkadan aydınlanan bir gaz devi
gal-saver-still-three-fireflies = Üç ateşböceği
gal-saver-still-three-fireflies-h = Üç uçuşun uzun pozlaması

## Bildirimler

notify-missed = Oyun modunda { $value } bildirim bekletildi
launcher-calc = { $query } = { $value }

## Ayarlar uygulaması (tasarım sayfasının yazmadıkları)

set-pick-images = Resimler
set-pick-folder = Klasör seç
set-refused = Bu değer kaydedilemedi
set-px = { $value } px
set-remove = Kaldır
set-add = Ekle
set-cancel = Vazgeç
mod-taskbar = Uygulamalar
mod-app-grid = Tüm uygulamalar düğmesi
set-bg = Arka plan
set-bg-h = Duvar kağıdının önünde barın arkasında ne dursun
set-bg-solid = Düz
set-bg-blur = Bulanık
set-bg-off = Yok
set-height = Yükseklik
set-height-h = Barın kalınlığı
set-height-default = Temanın kendi değeri: { $value } px
set-tray-shown = Bardaki tepsi simgeleri
set-tray-shown-h = Diğerleri taşma düğmesinden açılır
set-rule-exe = Program
set-rule-exe-ph = ör. Discord.exe
set-rule-float = Yüzen
set-rule-ignore = Yok say, hiç dizme
set-rule-workspace = Çalışma alanı { $value }
set-rule-monitor = Monitör { $value }
set-rule-monitor-main = Monitör { $value } (ana)
set-rule-monitor-away = Monitör { $value } (bağlı değil)
set-rule-min-width = en az { $value } px
set-rule-no-grab = Alt + tıklama programda kalır
set-rule-none = Henüz kendi kuralın yok
set-edit = Düzenle
set-save = Kaydet
set-refused-by-rule = { $value } kuralın
set-rule-match = Hangi pencereler
set-rule-match-h = Doldurduğun her alan eşleşmeli; düz metin adın herhangi bir yerinde aranır, büyük küçük harf fark etmez
set-rule-class = Pencere sınıfı
set-rule-class-ph = ör. Chrome_WidgetWin_1
set-rule-title = Başlık
set-rule-title-ph = ör. Resim içinde resim
set-rule-regex = Düzenli ifadeler
set-rule-regex-h = Alanlar düz metin değil, düzenli ifade olarak okunur
set-rule-do = Ne olsun
set-rule-do-h = En az bir şey
set-rule-float-h = Dizilmek yerine yüzerler
set-rule-ignore-h = Kabuk onlara dokunmaz
set-rule-no-grab-h = Alt + tıklamalar programa ulaşır; Kabuk pencerelerini tutmaz
set-rule-ws = Çalışma alanı
set-rule-ws-h = Bu çalışma alanında açılırlar
set-rule-mon = Monitör
set-rule-mon-h = Bu monitörde açılırlar; numarası Windows'taki gibi
set-rule-min = En küçük genişlik
set-rule-min-h = Dizilince en az bu kadar piksel alırlar
set-rule-place = Yerleşim
set-rule-place-h = Yüzerken monitörde nereye gidecekleri
set-rule-place-left = Sol
set-rule-place-right = Sağ
set-rule-place-top = Üst
set-rule-place-bottom = Alt
set-rule-place-center = Orta
set-rule-any = Fark etmez
set-rule-range = Bir tam sayı, { $value }
set-rule-need-match = Bir program, sınıf ya da başlık yaz
set-rule-need-action = Olacak en az bir şey seç
set-rule-pick-wait = Açık pencereler aranıyor…
set-rule-pick-none = Açık pencere yok
set-game-note = Kabuk oyunlara kod enjekte etmez ve genel kanca kurmaz; hile önleme sistemleri sıradan bir pencere görür.
set-src-windows = Açık pencereler
set-src-apps = Uygulamalar
set-src-recent = Son dosyalar
set-src-files = Dizinlenen klasörlerdeki dosyalar
set-src-bookmarks = Yer imleri
set-src-commands = Komutlar
set-src-calculator = Hesap makinesi
set-folder-add = Klasör ekle
set-key-launcher = Başlatıcıyı aç
set-key-workspace = Çalışma alanına geç (1…9 ile)
set-key-move-to-workspace = Pencereyi çalışma alanına taşı (1…9 ile)
set-key-focus-left = Odak: sol
set-key-focus-down = Odak: alt
set-key-focus-up = Odak: üst
set-key-focus-right = Odak: sağ
set-key-move-left = Pencereyi sola taşı
set-key-move-down = Pencereyi aşağı taşı
set-key-move-up = Pencereyi yukarı taşı
set-key-move-right = Pencereyi sağa taşı
set-key-toggle-float = Pencereyi yüzdür ya da diz
set-key-close-window = Pencereyi kapat
set-key-cycle-layout = Yerleşimi değiştir
set-key-toggle-scratchpad = Gizli çalışma alanını aç / kapat
set-key-to-scratchpad = Pencereyi gizli çalışma alanına gönder
set-key-toggle-group = Sekmeli grup yap / grubu dağıt
set-key-next-tab = Gruptaki sonraki sekme
set-key-overview = Tüm çalışma alanlarına genel bakış
set-key-start-saver = Ekran koruyucuyu hemen başlat
set-key-keys-card = Kısayol kartını göster
keys-title = Kısayollar
keys-title-h = Kabuk'un ana kısayolları, senin ayarladığın gibi
keys-welcome = Kabuk'a hoş geldin
keys-welcome-h = Pencerelerin kendiliğinden dizilir. Başlamak için bu kısayollar yeter.
keys-foot = Bir tuşa basmak ya da tıklamak kartı kapatır · Ayarlar › Kısayollar'dan değiştirebilirsin, bu kartınkini de
keys-g-windows = Pencereler
keys-g-workspaces = Çalışma alanları
keys-g-kabuk = Kabuk
keys-focus = Odağı taşı
keys-move = Pencereyi taşı
keys-workspace = Çalışma alanına geç
keys-move-to = Pencereyi çalışma alanına gönder
keys-emergency = Acil çıkış (her zaman çalışır)
set-key-app-grid = Tüm uygulamaları göster
set-bar-color = Bar rengi
set-bar-color-h = Arka plan düz ya da bulanıkken barı boyar
set-quiet = Kabuk'un bildirimlerini beklet
set-quiet-h = Rahatsız etme: kapatınca tek bir özet olarak gelirler; Kabuk'un yeniden başlaması yine gösterilir
set-bar-color-own = Temanın kendi rengi
set-bar-color-custom = İstediğin renk
set-bar-color-low = Temanın yazıları bu renk üstünde zor okunur
set-key-drag = Fareyle taşı (sağ tuş boyutlandırır)
set-key-press = Yeni kısayola bas…
set-reset-keys = Tüm kısayolları varsayılana döndür
set-key-mods = Basılı tutulacak tuşlara bas…
set-version = Kabuk { $value }
set-mode-dark-h = Temanın koyu renkleri, gün boyu
set-mode-light-h = Temanın açık renkleri, gün boyu
set-mode-system-h = Windows nasıl ayarlıysa koyu ya da açık
set-mem = MB bellek
set-not-running = Kabuk çalışmıyor
set-quit = Kabuk'u kapat
set-quit-h = Bar ve pencere yöneticisi durur; gizli pencereler, görev çubuğu ve Kabuk'un değiştirdiği her şey geri gelir
set-quit-btn = Kapat
set-update-checking = GitHub'da yeni bir Kabuk sürümü aranıyor…
set-update-latest = Kabuk güncel
set-update-newer = Kabuk { $value } çıktı: kurulumunu indirip çalıştır; bu sürümün üzerine kurulur, ayarların kalır
set-update-get = Sürüm sayfasını aç
set-update-failed = GitHub'a ulaşılamadı; sonra yeniden dene
set-store-missing = Microsoft Store sürümünde yok: Windows'un kendi ayarlarına yazıyor, Store buna izin vermiyor. GitHub'daki sürümde var.
set-update-store = Kabuk Microsoft Store'dan kuruldu; onu Store güncel tutar
set-cursors = Fare imleçleri
set-cursors-h = Kabuk'un imleçleri temaya ya da seçimine uyar; Kabuk kapanınca seninkiler geri gelir
set-cursors-windows = Kendi imleçlerin
set-cursors-kabuk = Temanın seti
set-cursors-custom = Benim seçimim
set-cursor-style = İmleç stili
set-cursor-color = İmleç rengi
set-cursor-size = İmleç boyutu
set-cursor-size-h = Kabuk'un imleçleri kullanılırken Windows'un işaretçi boyutu
cursor-style-round = Yuvarlak
cursor-style-slim = İnce
cursor-style-circuit = Devre
cursor-style-bold = Kalın
cursor-style-block = Blok
cursor-style-cartoon = Çizgi film
cursor-style-comic = Çizgi roman
cursor-color-light = Açık
cursor-color-dark = Koyu
cursor-color-accent = Vurgu rengi
cursor-size-normal = Normal
cursor-size-large = Büyük
cursor-size-huge = Çok büyük
set-animations = Animasyonlar
set-animations-h = Windows'un animasyon efektleri kapalıysa ve oyun modunda kapalı
set-animations-full = Tam
set-animations-reduced = Az
set-animations-off = Kapalı
set-sys-theme = Sistem teması
set-sys-colors = Renkler
set-sys-colors-h = Windows'un modu ve vurgu rengi temayı izler; Kabuk kapanınca seninkiler geri gelir
set-sys-colors-follow = Windows'un vurgu rengi temayı izler; tema Windows'a uyduğu için mod seninki kalır
set-drift = Sabit resmin kaysın
set-drift-h = Kendi duvar kâğıdı resmin her { $value } dakikada iki piksel kayar; göze görünmez, hiçbir piksel aynı ışıkta kalmaz
set-follow-look = Pencereler temaya uysun
set-follow-look-h = Diğer uygulamaların başlık çubukları temanın rengini ve köşelerini alır
set-minimize = Küçültme efekti
set-minimize-h = Pencerelerin bardaki simgelerine nasıl girip çıkacağı; animasyonlar tamken
set-minimize-genie = Cin
set-minimize-windows = Windows'unki
set-workspaces = Çalışma alanları
set-workspaces-h = Kaç tane olduğu; her biri tüm monitörlere yayılır
set-shown-workspaces = Bardaki çalışma alanları
set-shown-workspaces-h = Hep görünür; dolu olanlar ve etkin olan da görünür
set-ws-names = Çalışma alanı adları
set-ws-names-h = Barın ipuçlarında, genel bakışta, başlatıcıda ve önizlemelerde; boş olan numarasıyla kalır
set-ws-name = Çalışma alanı { $value }
set-tiling-layout = Dizme düzeni
set-tiling-layout-h = Her çalışma alanının düzeni; orada düzen kısayoluyla değiştirilmediyse
set-layout-dwindle = İkiye bölerek
set-layout-master = Ana pencere
set-double-click = Çift tıklayınca diz
set-double-click-h = Yüzen pencerenin başlığına çift tıklamak onu yeniden dizer; Windows'un büyütmesinin yerine
set-rule-defaults = Varsayılan kurallar
set-rule-defaults-h = Tabloda Varsayılan yazan kurallar; kapalıyken yalnız seninkiler geçerli
set-drift-every = Kayma aralığı
set-drift-every-h = Resmin iki adımı arasındaki dakika
tabs-untitled = Adsız pencere
w-windows = Windows'un duvar kağıdı
w-windows-h = Senin seçtiğin · Kabuk dokunmaz
day-long-0 = Pazar
day-long-1 = Pazartesi
day-long-2 = Salı
day-long-3 = Çarşamba
day-long-4 = Perşembe
day-long-5 = Cuma
day-long-6 = Cumartesi
month-long-0 = Ocak
month-long-1 = Şubat
month-long-2 = Mart
month-long-3 = Nisan
month-long-4 = Mayıs
month-long-5 = Haziran
month-long-6 = Temmuz
month-long-7 = Ağustos
month-long-8 = Eylül
month-long-9 = Ekim
month-long-10 = Kasım
month-long-11 = Aralık
date-long = { $day } { $month } { $weekday }
saver-settings-elsewhere = Ekran koruyucunun ayarları Kabuk Ayarlar'da, Ekran koruyucu sayfasında.

## Oluştur (Duvar kâğıdı sayfasının Oluştur sekmesi)

gen-name-claude = Claude
gen-name-openai = OpenAI
gen-name-ollama = Ollama
gen-why-reply = yanıtta duvar kâğıdı dosyaları yoktu
gen-why-format = dosyaları duvar kâğıdı biçimine uymadı
gen-why-compile = shader derlenmedi
gen-why-slow = 4K'da bir kare { $value } ms sürdü (sınır 4 ms)
gen-why-draw = bu bilgisayarın GPU'sunda çizilemedi
gen-why-black = arka plan siyah değildi
gen-why-bright = ortalamada fazla parlaktı (%{ $value }, sınır %8)
gen-why-static = parlak bir alan yerinde kaldı
gen-fail-no-key = Önce bir API anahtarı ekleyin
gen-fail-key = API anahtarı reddedildi
gen-fail-no-model = Ollama'da kurulu model yok
gen-fail-network = { $value } ile bağlantı kurulamadı
gen-fail-api = Model hata verdi ({ $value })
gen-fail-unreadable = { $value } yanıtı okunamadı
gen-fail-refused = Üç deneme de geçemedi. Son sebep: { $value }
gen-fail-cancelled = Durduruldu
gen-key-ph = API anahtarını yapıştırın
gen-key-save = Anahtarı kaydet
gen-try = Deneme { $value }/3
gen-step-perf = Performans: 4K'da kare başına { $value } ms (sınır 4 ms)
gen-step-oled = OLED: ortalama parlaklık %{ $value }, yerinde kalan parlak alan yok
gen-note = Duvar kâğıdını model yazar; Kabuk denetler, geçemeyeni düzeltilmesi için modele geri gönderir (en çok üç kez).
gen-stop = Durdur
gen-ollama-missing = Ollama { $value } adresinde yanıt vermedi
gen-ollama-model = Bağlı · yerel model { $value }
gen-ollama-url = Adres
gen-ollama-url-h = Ollama'nın bu bilgisayarda ya da ağında yanıt verdiği yer
gen-ollama-at = { $value } adresinde yerel model

## Kurulum (kabuk --install, --uninstall, --startup)

setup-title = Kabuk
setup-shortcut-settings = Kabuk Ayarları
setup-ask-startup = Kabuk her Windows oturumu açtığında başlasın mı? Bunu sonra "kabuk --startup on" ya da "off" ile değiştirebilirsin.
setup-installed = Kabuk { $path } klasörüne kuruldu. Başlat menüsünden başlatabilirsin.
setup-failed = Kabuk kurulamadı: { $error }
setup-removed = Kabuk kaldırıldı, Windows eski hâlinde. Ayarların { $path } klasöründe duruyor.
setup-remove-failed = Kabuk tamamen kaldırılamadı: { $error }
setup-startup-on = Kabuk oturum açınca başlayacak.
setup-startup-off = Kabuk oturum açınca başlamayacak.
setup-startup-blocked = Windows ayarları Kabuk'un oturum açılınca başlamasını engelliyor; Ayarlar › Uygulamalar › Başlangıç'tan aç.
store-description = Windows için bir masaüstü kabuğu: her monitörde bir bar, döşenen pencereler, bir başlatıcı ve kendi temaları. Kapanınca Windows eski hâline döner.
store-listing-description =
    Kabuk, Windows'a yeni bir masaüstü getirir: her monitörde kendi barı, kendiliğinden düzenlenen pencereler, hızlı bir başlatıcı ve seçilecek on tema. Kabuk çalışırken Windows görev çubuğu kenara çekilir; Kabuk kapanınca Windows tam olarak eski hâline döner.

    Pencerelerin yan yana döşenir ya da bıraktığın yerde yüzer; tüm monitörlerine yayılan dokuz çalışma alanı vardır. Onları klavyeden taşı, boyutlandır ve değiştir ya da Alt'a basılı tutup herhangi bir yerinden tut. Küçültülen pencereler bardaki simgelerine akar.

    Bar çalışma alanlarını, uygulamalarını, sistem tepsisini, bildirimleri, medyayı, sesi, ağı, pili, saati ve bilgisayarın yükünü gösterir; Wi-Fi, hızlı ayarlar, takvim ve bildirimler için kendi panelleri vardır. Başlatıcı yazdıkça uygulamaları, açık pencereleri, dosyaları ve yer imlerini bulur, hesap da yapar; Tüm uygulamalar kurulu her şeyi listeler.

    Her tema barı, panellerini ve duvar kâğıdını birlikte değiştirir; koyu ya da açık. Hareketli duvar kâğıtları OLED ekranlar için çizilir: gerçek siyah, yerinde duran parlak hiçbir şey yok. İstersen bir duvar kâğıdını anlat, seçtiğin bir yapay zekâ modeli çizsin.

    Oyun modu, tam ekran bir oyun açıkken kenara çekilir. Bir şey ters giderse Ctrl+Alt+Shift+End her pencereyi ve Windows görev çubuğunu bir anda geri getirir.

    Kabuk ücretsiz ve açık kaynaktır (Apache 2.0). Senin hakkında hiçbir şey toplamaz.
store-listing-feature-1 = Her monitörde bir bar: çalışma alanları, uygulamalar, tepsi, ağ, pil, bildirimler, takvim, medya, hızlı ayarlar
store-listing-feature-2 = Monitörlerine yayılan dokuz çalışma alanında kendiliğinden döşenen ya da yüzen pencereler
store-listing-feature-3 = Uygulamalar, pencereler, dosyalar ve yer imleri için bir başlatıcı ve kurulu her şey için Tüm uygulamalar
store-listing-feature-4 = Koyu ve açık on tema: bar, paneller ve duvar kâğıdı birlikte
store-listing-feature-5 = OLED ekranlar için yapılmış hareketli duvar kâğıtları
store-listing-feature-6 = Cin efekti: pencereler küçülürken simgelerine akar
store-listing-feature-7 = Tam ekran oyunlar için kenara çekilen oyun modu
store-listing-feature-8 = Ctrl+Alt+Shift+End Windows'u her an geri getirir
store-listing-keywords = masaüstü kabuğu, döşemeli pencere yöneticisi, görev çubuğu, başlatıcı, çalışma alanları, temalar, OLED
setup-store-managed = Kabuk Microsoft Store'dan kuruldu: onu Windows kurar, günceller ve kaldırır (Ayarlar › Uygulamalar).
setup-confirm = Kabuk senin için kurulacak (yönetici izni gerekmez). Çalışırken Windows görev çubuğunun yerini kendi barı alır ve pencerelerini düzenler; kapanınca Windows eski hâline döner. Bir şey ters giderse Ctrl+Alt+Shift+End her şeyi geri getirir. Kabuk şimdi kurulsun mu?
setup-done = Kabuk kuruldu ve çalışıyor. Başlat menüsündeki "Kabuk Ayarları" görünümünü ve davranışını değiştirir; masaüstündeki "Kabuk" kısayolu onu yeniden başlatır. Terminalde "kabuk --help" komutlarını listeler. Ctrl+Alt+Shift+End Windows'u her an geri getirir.
setup-no-payload = Bu kurulumun içinde program yok (onlarsız derlenmiş). tools\package.ps1 ile derle.

# kabuk.toml'daki yanlışlar (bildirimler ve Ayarlar; PLAN M19 U1-U3).
cfg-at-line = Satır { $line }: { $what }
cfg-syntax = dosya geçerli bir TOML değil ({ $detail })
cfg-invalid = bu değer kullanılamıyor ({ $detail })
cfg-schema = dosya daha yeni bir Kabuk için (şema { $found }; bu sürüm { $supported } okuyor)
cfg-range = { $min } ile { $max } arasında olmalı
cfg-one-of = şunlardan biri olmalı: { $list }
cfg-color = { $value } bir renk değil (#rrggbb ya da "theme")
cfg-shortcut-taken = { $chord } zaten "{ $other }" için kullanılıyor
cfg-regex = geçerli bir düzenli ifade değil ({ $detail })
cfg-url = { $value } bir http(s) adresi değil
cfg-rule-match = kuralın bir exe, sınıf ya da başlığa ihtiyacı var
cfg-rule-action = kuralın yapacak bir şeyi yok
cfg-missing = burada bir değer gerekiyor
cfg-empty = değer boş
cfg-duplicate = değer iki kez yazılmış
cfg-unknown-key = "{ $key }" bir ayar değil; yok sayıldı
cfg-no-schema = "schema" yok; şimdiki sürüm varsayıldı
cfg-key-empty = kısayol boş
cfg-key-unknown = "{ $key }" bir tuş değil
cfg-key-not-modifier = "{ $key }" basılı tutulamaz (Ctrl, Alt, Shift ya da Win)
cfg-key-twice = "{ $key }" kısayolda iki kez geçiyor
cfg-key-order = "{ $key }" tuştan sonra geliyor; önce o basılı tutulur
cfg-key-two-keys = kısayolda basılı tutulanların yanında tek tuş olur
cfg-key-no-key = kısayolda basılı tutulanların yanında tuş yok
cfg-key-needs-modifier = kısayola Ctrl, Alt ya da Win gerekir
cfg-key-reserved = bu kısayolu Windows ya da Kabuk kendine ayırmış
cfg-key-altgr = Türkçe klavyede harfle Ctrl+Alt, AltGr demektir
notify-config-rejected = kabuk.toml'da bir yanlış var
notify-config-defaults = Varsayılan ayarlar kullanılıyor. { $where }
notify-config-kept = Son çalışan ayarlar duruyor. { $where }
notify-config-unknown = kabuk.toml'da Kabuk'un tanımadığı bir ayar var
notify-config-unknown-s = Yok sayıldı. { $where }
notify-config-line = Satır { $value }.
set-problems = kabuk.toml okunamadı, bu yüzden bunlar varsayılan ayarlar. Dosyayı düzeltince buradaki değişiklikler yine kaydedilir:
set-open-config = kabuk.toml'u aç
set-search = Ayarlarda ara
set-search-title = Arama
set-search-count = { $value } sonuç
set-search-none = Eşleşen bir ayar yok; başka bir sözcük dene
set-file = Ayar dosyası
set-file-h = Bu sayfalarda ayarlanan her şey kabuk.toml'da tutulur
set-file-kind = Kabuk ayarları
set-file-export = Ayarları dışa aktar
set-file-export-h = kabuk.toml'u saklamak ya da başka bir bilgisayara götürmek için seçtiğin bir dosyaya kaydeder
set-file-export-btn = Dışa aktar…
set-file-import = Ayarları içe aktar
set-file-import-h = Kaydedilmiş bir dosyayı buradaki ayarların yerine okur; Kabuk'un okuyamadığı bir dosya alınmaz
set-file-import-btn = İçe aktar…
set-file-exported = Kaydedildi: { $value }
set-file-imported = Ayarlar dosyadan okundu. Öncekiler { $value } dosyasında duruyor
set-file-refused = { $value } alınmadı; Kabuk onu okuyamıyor:
set-file-unreadable = Dosya okunamadı: { $value }
set-file-not-saved = Dosya kaydedilemedi: { $value }
set-file-undo = Önceki ayarları geri getir
set-file-undone = Önceki ayarlar geri geldi. Bıraktıkların { $value } dosyasında duruyor
set-reset-all = Hepsini sıfırla
set-reset-all-h = Her ayar varsayılanına döner
set-reset-all-btn = Hepsini sıfırla…
set-reset-all-q = Her ayar varsayılanına dönsün mü?
set-reset-all-d = Kısayollar ve pencere kuralları da varsayılanlarına döner. Düzen profillerin kalır; ayar olmayan Tüm uygulamalar düzeni, oluşturduğun duvar kâğıtları ve API anahtarların da kalır. kabuk.toml önce kabuk.toml.bak olarak kaydedilir, böylece ayarlar geri getirilebilir.
set-reset-all-yes = Hepsini sıfırla
set-reset-done = Her ayar varsayılanına döndü. Öncekiler { $value } dosyasında duruyor
set-key-windows-hint = Windows da { $value } kullanıyor; Kabuk çalışırken bu kısayol onun yerine Kabuk'un işini yapar.

# İstenen bir şey olmadı (PLAN M19 U6).
notify-failed-open = Açılamadı
notify-failed-command = Windows bunu yapmadı
notify-failed-settings = Ayarlar açılamadı
notify-failed-switch = Değişmedi
notify-failed-brightness = Parlaklık değişmedi
notify-failed-output = Ses çıkışı değişmedi
notify-failed-dropdown = Açılır terminal başlamadı
notify-failed-copy = Kopyalanamadı
notify-failed-s = Nedeni Kabuk'un günlüğünde.

# Döşeme düzeni değişti (PLAN M19 U7).
notify-layout-dwindle = Bölünen düzen
notify-layout-dwindle-s = Her yeni pencere sondakini ikiye böler.
notify-layout-master = Ana pencere düzeni
notify-layout-master-s = Bir büyük pencere, öbürleri yanında bir sütunda.

# Kabuk durduktan sonra döndü; yönetici olarak çalışan program yüzer (PLAN M19 U12, U13).
notify-restarted = Kabuk yeniden başladı
notify-restarted-s = Beklenmedik biçimde durdu; nedeni günlüğünde.
notify-elevated = Bir program yönetici olarak çalışıyor
notify-elevated-s = Kabuk onun pencerelerini taşıyamaz, bu yüzden yüzerler. Onları da düzenlemesi için Kabuk'u yönetici olarak başlat.
notify-quiet-off = Rahatsız etme kapandı
notify-quiet-missed = { $value } bildirim bekletildi
notify-already-running = Kabuk zaten çalışıyor
notify-already-running-s = Yeniden başlatıldı; çalışan Kabuk sürüyor.
notify-started-again = Kabuk zaten çalışıyordu
notify-started-again-s = Yeniden başlatıldı ve çalışanın yerini aldı.

# Bar modülünün ipucu (PLAN M19 U15).
tip-workspace = Çalışma alanı { $value }
workspace-named = Çalışma alanı { $value } · { $name }

# Ekran okuyucunun bir bar modülü için söylediği: adı, ardından değeri (DECISIONS D97).
acc-part = { $label }: { $value }
# Medya kartının yalnızca resimle çizilen düğmeleri.
acc-media-previous = Önceki parça
acc-media-toggle = Oynat veya duraklat
acc-media-next = Sonraki parça

# Başlatıcı, girdileri toplanırken (PLAN M19 U20).
l-searching = Aranıyor…
