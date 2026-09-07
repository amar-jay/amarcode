# İçindekiler

## 1. Kapak

- Raporun adı
- Projenin adı: Amarcode
- Öğrencinin adı ve soyadı
- Öğrenci numarası
- Üniversite ve bölüm
- Staj yapılan kurum
- Staj tarihleri
- Kurum ve akademik danışman bilgileri
- Teslim tarihi

## 2. İçindekiler

- Raporun bölüm ve alt bölümlerinin sayfa numaralarıyla listelenmesi
- Şekiller listesi
- Tablolar listesi
- Kısaltmalar listesi

## 3. Özet

- Stajın amacı ve kapsamı
- Amarcode projesinin çözdüğü problem
- Kullanılan temel teknolojiler
- Staj süresince gerçekleştirilen çalışmalar
- Elde edilen teknik ve mesleki kazanımlar
- Projenin ulaştığı sonuç

## 4. İş Yeri Tanıtımı

- Kurumun adı ve genel bilgileri
- Kurumun faaliyet alanı
- Kurumun organizasyon yapısı
- Yazılım geliştirme ekibi ve kullanılan çalışma yöntemleri
- Staj yapılan birimin görevleri
- Stajyerin ekip içindeki rolü ve sorumlulukları

## 5. Staj Süresince Yapılan Çalışmalar

### 5.1. Projenin Amacı ve Kapsamı

- Amarcode projesinin tanımı
- Hedef kullanıcı kitlesi
- Temel ürün gereksinimleri
- Yapay zekâ kodlama ajanlarının tek masaüstü uygulamasında birleştirilmesi

### 5.2. Kullanılan Teknolojiler

- React ve TypeScript
- Tauri
- Rust ve Cargo çalışma alanı
- SQLite
- Jotai
- Bun
- Cloudflare Workers
- Agent Client Protocol
- JSON-RPC
- Git ve GitHub Actions

### 5.3. Sistem Mimarisi

- React kullanıcı arayüzü
- Tauri masaüstü köprüsü
- Amarcode daemon
- Ortak istemci–daemon protokolü
- SQLite kalıcı veri katmanı
- ACP ajan süreçleri
- Ajan ve daemon kayıt sistemleri

### 5.4. Kullanıcı Arayüzü Çalışmaları

- Yeni sohbet ve istem giriş ekranı
- Açık ve koyu tema
- Sohbet geçmişi ve kenar çubuğu
- Ajan seçimi ve ayarlar
- Akışlı mesajlar, kod blokları ve düşünme bölümleri
- Dosya ağacı, kaynak görüntüleme ve diff inceleme

### 5.5. Daemon ve Kalıcı Veri Çalışmaları

- TCP JSON-line RPC sunucusu
- Sohbet, mesaj, çalışma ve ajan kayıtları
- “Önce kaydet, sonra bildir” ilkesi
- Bağlantı kesilmesi ve yeniden bağlanma
- Eş zamanlı çalışma ve sahiplik kontrolleri
- Kullanıcı servisi ve tek daemon örneği

### 5.6. ACP ve Ajan Entegrasyonu

- ACP istemcisi ve oturum yönetimi
- Ajan mesajlarının ürün olaylarına dönüştürülmesi
- İptal davranışı
- Araç çağrıları
- Kullanıcı izinleri
- Terminal oluşturma, çıktı alma ve temizleme
- Ajan kimlik doğrulaması

### 5.7. Registry, Dağıtım ve Güncelleme

- Ajan manifestlerinin eşitlenmesi
- Ajan kullanılabilirlik denetimi
- Ajan kurulum süreci
- Daemon sürüm kontrolü
- İndirme ve bütünlük doğrulaması
- Güncelleme, sağlık kontrolü ve geri alma
- Windows paketleme ve kaldırma işlemleri

### 5.8. Test ve Doğrulama

- React durum testleri
- ACP transcript testleri
- Daemon dikey dilim testleri
- Ortak protokol ve TypeScript bağ kontrolü
- Registry Worker testleri
- Lint, biçimlendirme ve derleme kontrolleri
- Platforma özgü hata düzeltmeleri

## 6. Günlük Raporlar (Gün Gün)

### 6.1. 1. Gün — Projenin İncelenmesi ve Mimari Planlama

[12 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/12.08.2026.md)

### 6.2. 2. Gün — Masaüstü Arayüzünün ve İstem Girişinin Oluşturulması

[13 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/13.08.2026.md)

### 6.3. 3. Gün — Rust Daemon, TCP İletişimi ve RPC Temeli

[14 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/14.08.2026.md)

### 6.4. 4. Gün — Ön Yüz ile Daemon Arasında Canlı Veri Akışı

[15 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/15.08.2026.md)

### 6.5. 5. Gün — Sohbet Oturumları, İyimser Güncellemeler ve Durum Yönetimi

[17 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/17.08.2026.md)

### 6.6. 6. Gün — Akışlı Mesajlar, Kod Gösterimi ve Değişiklik İnceleme

[18 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/18.08.2026.md)

### 6.7. 7. Gün — Eş Zamanlılık, Eski Olaylar ve Yeniden Bağlanma

[19 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/19.08.2026.md)

### 6.8. 8. Gün — Ortak Protokol, Tür Üretimi ve Uyumluluk Kontrolü

[20 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/20.08.2026.md)

### 6.9. 9. Gün — SQLite Kalıcılığı ve Önce Kaydetme İlkesi

[21 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/21.08.2026.md)

### 6.10. 10. Gün — Daemon Yaşam Döngüsü ve İşletim Sistemi Servisi

[22 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/22.08.2026.md)

### 6.11. 11. Gün — Daemon Güncelleme, Doğrulama ve Kaldırma Süreci

[24 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/24.08.2026.md)

### 6.12. 12. Gün — Çalışma Alanı Gezgini, Arama, Ekler ve Diff İnceleme

[25 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/25.08.2026.md)

### 6.13. 13. Gün — Agent Client Protocol Entegrasyonu

[26 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/26.08.2026.md)

### 6.14. 14. Gün — Araç Çağrıları ve Kullanıcı İzinleri

[27 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/27.08.2026.md)

### 6.15. 15. Gün — Terminal Yaşam Döngüsü ve Güvenli Komut Çalıştırma

[28 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/28.08.2026.md)

### 6.16. 16. Gün — Ajan Kayıt Sistemi, Keşif ve Kurulum

[29 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/29.08.2026.md)

### 6.17. 17. Gün — Ajan-Bağımsız Yapılandırma ve Kimlik Doğrulama

[31 Ağustos 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/31.08.2026.md)

### 6.18. 18. Gün — Son Entegrasyon, Testler ve Staj Değerlendirmesi

[1 Eylül 2026 tarihli günlük rapor](https://github.com/amar-jay/amarcode/blob/main/reports/01.09.2026.md)

## 7. Sonuç

- Staj sürecinin genel değerlendirmesi
- Projede ulaşılan teknik sonuç
- Kazanılan bilgi ve beceriler
- Karşılaşılan güçlüklerin mesleki gelişime katkısı
- Projenin kullanılabilirlik ve sürdürülebilirlik değerlendirmesi
- Gelecekte yapılabilecek geliştirmeler

## 8. Kaynakça

[Amarcode Staj Raporları — Toplu Kaynakça](https://github.com/amar-jay/amarcode/blob/main/reports/KAYNAKCA.md)

Kaynakça aşağıdaki grupları kapsamaktadır:

- Amarcode proje ve kaynak kodu belgeleri
- Oluşturulan teknik diyagramlar
- Kullanılan teknolojilerin resmî dokümantasyonu
- Protokol ve teknik standartlar
- Güvenlik kaynakları
- SQLite dokümantasyonu

## 9. Ekler

[Amarcode Staj Raporu — Ekler](https://github.com/amar-jay/amarcode/blob/main/reports/EKLER.md)

Ekler aşağıdaki materyalleri kapsamaktadır:

- Proje bileşenleri ve depo yapısı
- Genel sistem mimarisi
- İstem ve kalıcı veri akışları
- ACP araç izni ve terminal yönetimi
- Daemon ve registry yaşam döngüleri
- Oturum yapılandırma akışı
- Seçilmiş uygulama ekran görüntüleri
- RPC ve canlı olay türleri
- Test ve doğrulama matrisi
- Git tabanlı geliştirme özeti
- Bilinen sınırlamalar ve gelecek çalışmalar

