# Paint ile Mona Lisa

Bu örnek resmi Paint'e yapıştırmaz. PointHand kendi MCP sunucusuyla renk seçer ve fare girdileriyle 24.575 ayrı çizgi çizer. Referans kamu malı Mona Lisa eseridir; sonuç ve demo proje sahibi tarafından paylaşılmıştır.

![PointHand ile çizilen sonuç](result.png)

[Çizim videosu](demo.mp4). Paylaşım kopyası ses ve kaynak metadata içermez. PNG kopyalarındaki metadata temizlenmiştir; kaynak dosyalar değiştirilmemiştir.

Depo kökünde Node.js bağımlılıklarını kurun ve yerel Rust sunucusunu derleyin:

```powershell
cd client
npm ci
npm run build
cd ..
./scripts/build-windows.ps1
```

Paint'te yeni, boş **430 × 640 piksel** tuval açın. Yakınlaştırmayı %100 yapın, çizgi kalemini ve **2 piksel** boyutu seçin. Çizim sırasında Paint'i taşımayın veya yeniden boyutlandırmayın.

PointHand `list_windows` aracından Paint pencere kimliğini alın. `get_window_state` ekran görüntüsünde tuvalin sol üst noktasını, **pencereye göre fiziksel piksel** olarak belirleyin. Kimlik ve koordinatları başka bir oturumdan kopyalamayın.

```powershell
node scripts/paint-image.mjs <pencere-kimliği> examples/mona-lisa/strokes.json <tuval-sol> <tuval-üst>
```

Kaynak daldaki betik Türkçe adlara ek olarak İngilizce kontrol takma adlarını da içerir. Diğer diller veya farklı Paint sürümleri için kodu değiştirmeden, güncel PointHand erişilebilirlik ağacında gördüğünüz adları bir UTF-8 JSON dosyasına yazabilirsiniz:

```json
{
  "editColors": ["Edit colors"],
  "hex": ["Hex"],
  "confirm": ["OK"]
}
```

```powershell
node scripts/paint-image.mjs <pencere-kimliği> examples/mona-lisa/strokes.json <tuval-sol> <tuval-üst> --selectors paint-selectors.json
```

Bu seçenek Alpha 1 sonrasındaki kaynak dalda bulunur; mevcut Alpha 1 indirmesi eski betiği içerir. Türkçe/İngilizce eşleme testleri otomatik çalışır; İngilizce Paint üzerinde canlı doğrulama henüz yapılmamıştır. Ad eşleşmesi belirsizse veya kontrol görünür/etkin değilse betik durur.

Çizim yaklaşık 17 dakika sürmüştür; süre bilgisayara ve Paint sürümüne bağlıdır. Escape işlemi durdurur. Fiziksel fare/klavye müdahalesi etkin işlemi keser. İlerleme `.scratch/` klasörüne yazılır; bu klasör kişisel pencere bilgileri içerebileceğinden paylaşım paketine eklenmemelidir.

Hazır plan Python gerektirmez. Kendi referansınız için Pillow kurarak yeni plan üretebilirsiniz:

```powershell
python -m pip install Pillow
python scripts/image-stroke-plan.py examples/mona-lisa/reference.png .scratch/my-plan.json
```

Üretilen plan kaynak dosyanın yalnızca adını saklar; mutlak kullanıcı yolu içermez. Başka resimlerin paylaşım haklarını ayrıca değerlendirin.
