# Contributing to PointHand

[English](#english) · [Türkçe](#türkçe)

## English

PointHand welcomes code, reproducible bug reports, Windows compatibility tests, documentation and creative examples. You do not need to write Rust to help.

### Choose a starting point

- Browse [good first issues](https://github.com/ayzekhdawy/pointhand/labels/good%20first%20issue) for small tasks and [help wanted](https://github.com/ayzekhdawy/pointhand/labels/help%20wanted) for broader work.
- Comment on an issue before starting substantial work so contributors can coordinate. Small documentation fixes can go straight to a pull request.
- Discuss changes to input behavior, permissions, dependencies or architecture in an issue first.

### Set up a source checkout

Fork the repository, clone your fork and create a branch. Install Node.js 20+, stable Rust and Visual Studio Build Tools with Desktop development with C++.

```powershell
npm ci --prefix client
./scripts/build-windows.ps1
npm run build --prefix client
```

See the [technical reference](docs/technical-reference.md). Python with Tkinter is needed only for the optional reference implementation and interactive test fixtures.

### Validate your change

Run the checks relevant to your changes; explain what you ran in your PR. Documentation-only changes need link/content checks rather than desktop tests.

```powershell
npm test --prefix client
node --test tests/js/*.test.mjs
cargo fmt --manifest-path host/Cargo.toml --check
cargo test --manifest-path host/Cargo.toml
cargo clippy --manifest-path host/Cargo.toml -- -D warnings
node --test scripts/host-regression.mjs
```

Interactive tests move the real pointer. Use a disposable blank document, an app allow-list and a supervised session. See the [validation checklist](docs/release-checklist.md) before running them. Never test against another person's session or important unsaved work.

### Send a useful pull request

Keep one purpose per PR. Describe the problem, resulting behavior and validation. Add a regression test when it meaningfully protects changed logic. Include screenshots for visual changes after checking them for private information. Disclose any untested environments. AI-assisted contributions are welcome; contributors remain responsible for understanding and verifying their changes.

Do not commit `.scratch/`, build artifacts, credentials, local executable paths, private desktop captures or original personal media. Use synthetic fixtures and public assets. Do not publish key content or sensitive logs. Report security vulnerabilities through [SECURITY.md](SECURITY.md), not public issues.

By contributing, you agree to license your contribution under the project's [MIT license](LICENSE). Review and merge remain the maintainer's responsibility.

### Help without writing code

Use the compatibility-report issue template to share Windows/app versions, display layout, DPI, steps, expected behavior and actual results. Redact account names and notifications from screenshots. Share a creative workflow with its source, license, setup instructions and PointHand script. Label illustrative mockups separately from actual captures.

Be respectful, discuss the work and avoid personal attacks. Small, well-documented contributions are valuable.

## Türkçe

Kod yazmadan da katkı sağlayabilirsin: kurulum denemesi, hata raporu, farklı ekran/DPI testleri, çeviri ve yaratıcı örnekler de katkıdır.

1. [Başlangıç görevlerinden](https://github.com/ayzekhdawy/pointhand/labels/good%20first%20issue) birini seç. Büyük bir işe başlamadan önce issue'ya yorum yazarak haber ver.
2. Depoyu fork'la, kendi dalında çalış ve yukarıdaki kurulum adımlarını uygula.
3. Değişikliğinle ilgili testleri çalıştır. PR açıklamasında neyi değiştirdiğini, nasıl doğruladığını ve denemediğin ortamları belirt.
4. Masaüstü testlerini boş belgelerde, izin listesiyle ve gözetim altında yap. Bu testler gerçek fareyi hareket ettirir.
5. Kişisel ekran görüntülerini, kullanıcı yollarını, erişim anahtarlarını veya özel logları paylaşma. Güvenlik açıkları için [güvenlik politikasını](SECURITY.md) izle.

Hata veya uyumluluk raporunu Türkçe ya da İngilizce açabilirsin. İşletim sistemi, uygulama sürümü, ekran ölçeği ve tekrar üretme adımlarını ekle. Katkılar MIT lisansıyla paylaşılır; herkesin katkısını saygılı biçimde değerlendiririz.
