# agent-desktop entegrasyonu

Kaynak: https://github.com/vercel-labs/agent-desktop (incelendi 31.10.2026).
Rust ile yazılmış, macOS/Windows/Linux için bir CLI ve C kütüphanesi. Masaüstünü
erişilebilirlik ağacı (Windows'ta UI Automation) üzerinden okur ve "ref" denen
öğe kimlikleriyle işlem yapar. 60 komut adı vardır.

## Lisans

- agent-desktop: **Apache-2.0**. PointHand: **MIT**.
- Apache-2.0 kodu MIT projeye kopyalanabilir, ama kopyalanan dosyalarda lisans
  metni, NOTICE ve değişiklik bildirimi tutulmalı.
- Karar: **kod kopyalanmadı.** agent-desktop çok sandıklı, kendi soyutlamaları olan
  büyük bir yapı (core/adapter/ref çözümleyici). PointHand'in tek dosyalı
  `desktop.rs` düzenine uymaz. Özellikler yalnız **fikir düzeyinde**, PointHand'in
  kendi adlandırmasıyla ve `uiautomation` / `windows` sandıklarıyla yeniden yazıldı.
  Fikir için teşekkür `CHANGELOG.md` ve teknik başvuruda belirtildi.

## Karşılaştırma

| agent-desktop | PointHand (önce) | Karar |
| :--- | :--- | :--- |
| `snapshot`, `screenshot` | `get_window_state`, `screenshot` | Var |
| `click`, `double-click`, `right-click` | `click` (1–2 tık), batch | Var |
| `triple-click` | Yok | **Eklendi:** `click_count` 3, batch `triple_click` |
| `toggle`, `check`, `uncheck` | Yok | **Eklendi:** `perform_secondary_action` "Toggle", ağaçta `checked`/`unchecked`/`mixed` |
| `select` | Yok (yalnız `selected` durumu) | **Eklendi:** "Select" eylemi |
| `expand`, `collapse`, `scroll-to` | "Expand", "Collapse", "Raise" | Var |
| `set-value`, `type`, `press` | `set_value`, `type_text`, `press_key` | Var |
| `find` (rol/ad/kimlikle ara) | Yok; ağaç 400 öğede kesiliyor | **Eklendi:** `find_elements` (3000 öğeye kadar tarar, yalnız eşleşenleri döndürür) |
| `wait` (öğe/pencere bekle) | Yok; yalnız batch `wait` (süre) | **Eklendi:** `wait_for` (pencere başlığı ya da öğe; görünene ya da kaybolana kadar) |
| `minimize`, `maximize`, `restore` | Yok | **Eklendi:** `set_window_state` |
| `move-window`, `resize-window` | Yok | **Eklendi:** `move_window` |
| `close-app` + korunan süreç listesi | Yok | **Eklendi:** `close_window` (kibar WM_CLOSE, korunan sistem süreçlerini reddeder) |
| Secure field (parola değeri gizli) | Parola kutusunun değeri ağaçta görünebiliyordu | **Eklendi:** parola alanının değeri yazılmaz, `password` etiketi gösterilir |
| Integrity boundary (yönetici penceresi) | Girdi sessizce düşüyordu | **Eklendi:** yükseltilmiş pencereye girdi göndermeden açık hata |
| Actionability (devre dışı öğe) | COM hatası | **Eklendi:** devre dışı öğede `set_value`/eylem açık hatayla reddedilir |
| Headless ref action (imleç ve odak kullanmadan) | Yok; her işlem imleci ve odağı alıyordu | **Eklendi (2. tur):** arka plan modu, `background: true` / `POINTHAND_BACKGROUND=1` |
| `get`, `is` (öğe özellikleri) | Yalnız ağaç satırı | **Eklendi (2. tur):** `get_element` |
| `focus` (öğe) | Yok | **Eklendi (2. tur):** "Focus" eylemi |
| `hover` | `move` | Arka planda da çalışıyor |
| `clipboard-*`, `launch`, `list-*` | Var | Var |
| Bildirimler (`list-notifications` …) | Yok | Ertelendi: Windows bildirim merkezi kararsız, düşük değer |
| `session`, `trace`, imleç katmanı | PointHand'in kendi katmanı var | Gerek yok |
| `key-down/up`, `mouse-down/up` | Yok | Bilerek yok: tutulan tuş güvenlik riski (agent-desktop da reddediyor) |
| Ref kimliği, eski ref tespiti | `element_index` + pencere denetimi | Ertelendi: büyük yeniden yazım gerekir |

## Arka plan modu (2. tur)

İshak'ın en çok istediği özellik: kullanıcı bir pencerede çalışırken ajan başka bir
pencerede çalışsın. Windows'ta tek bir fare imleci ve tek bir klavye odağı var; gerçek
fare iki yerde aynı anda olamaz. Bu yüzden arka plan modu imleci hiç kullanmaz:

- Erişilebilirlik eylemleri (`set_value`, `perform_secondary_action`) odağı almadan çalışır.
- Tıklama, sürükleme, çizim, kaydırma, yazı ve tek tuş pencereye Windows mesajı olarak
  gönderilir (PostMessage). Pencere başka pencerenin altında kalsa da çalışır; küçültülmüş olmamalı.
- "Kullanıcı boşta mı" beklemesi atlanır, çünkü fiziksel girdi yok.
- Korunanlar: izin listesi, yönetici penceresi reddi, kilitli masaüstü denetimi, Escape.
- Sınır: klasik Win32 uygulamaları mesajları alır; Chrome, Electron ve bazı yeni Windows
  uygulamaları yok sayabilir. Sonuç `delivery: "posted"` döner, ekran görüntüsüyle doğrulanır.
- `paste` (pano kullanıcınındır) ve Ctrl gibi değiştirici tuşlu kısayollar arka planda reddedilir.

Kod: `host/src/desktop/background.rs`.

## Nereye eklendi

- `host/src/desktop/elements.rs` (yeni): `find_elements`, `wait_for`, eşleştirme yardımcıları.
- `host/src/desktop/windows_ops.rs` (yeni): `set_window_state`, `move_window`,
  `close_window`, korunan süreç listesi, integrity denetimi.
- `host/src/desktop.rs`: Toggle/Select eylemleri ve etiketleri, `checked` durumu,
  parola gizleme, devre dışı öğe reddi, üçlü tık, batch eylemleri, guard'a integrity denetimi.
- `host/src/main.rs`: yeni yöntemler. `host/src/guard.rs`: salt okunur yöntemler girdi kilidi almaz.
- `client/src/index.ts`: 5 yeni MCP aracı, şema güncellemeleri.
- Belgeler: `README.md`, `docs/technical-reference.md`, `skills/pointhand/SKILL.md`, `CHANGELOG.md`.

## Güvenlik modeli korunuyor

- Değişen pencere işlemleri (`set_window_state`, `move_window`, `close_window`) `guard_input`
  üzerinden geçer: `POINTHAND_ALLOW` izin listesi, kullanıcı boşta bekleme, Escape.
- `find_elements` ve `wait_for` salt okunur; ama hedef pencere izin listesinde olmalı
  (`get_window_state` ile aynı kural). `wait_for` Escape ile durur, en çok 30 sn.
- Integrity denetimi yalnız **ek bir ret**tir; okunamazsa eski davranış sürer.
- `close_window` batch içinde yok; tek tek, açıkça çağrılır.

## Doğrulama

Derleme ve testler yerelde değil, GitHub Actions'ta (`.github/workflows/windows.yml`,
windows-latest): `npm test`, `cargo fmt --check`, `cargo test`, `cargo clippy -D warnings`,
paketleme ve paket doğrulama; ZIP artifact olarak yüklenir.
Canlı masaüstü denemesi (gerçek uygulamada Toggle/Select, pencere işlemleri) İshak'ın
gözetiminde ayrıca yapılmalı.
