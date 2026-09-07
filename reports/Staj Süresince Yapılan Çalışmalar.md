# Staj Süresince Yapılan Çalışmalar

## Projenin Amacı ve Kapsamı

Staj süresince **Amarcode** adlı, yapay zekâ destekli kodlama ajanlarını tek bir masaüstü uygulamasında birleştirmeyi amaçlayan yazılım projesi üzerinde çalıştım. Projenin temel amacı, kullanıcının bir yazılım çalışma alanı açabilmesi, farklı kodlama ajanlarından birini seçebilmesi, doğal dilde görev verebilmesi ve ajanın yaptığı işlemleri uygulama içinden takip edebilmesidir. Amarcode yalnızca metin tabanlı bir sohbet arayüzü olarak düşünülmemiştir. Sohbet geçmişinin kalıcı olarak saklanması, uzun görevlerin arka planda sürdürülebilmesi, ajan tarafından kullanılan araçların görüntülenmesi, dosya değişikliklerinin incelenmesi ve riskli işlemlerin kullanıcı iznine bağlanması projenin temel kapsamına dâhil edilmiştir.

Farklı yapay zekâ sağlayıcıları ve kodlama ajanları birbirlerinden farklı çalışma biçimlerine sahip olabildiği için uygulamanın tek bir sağlayıcıya bağlı kalmaması hedeflenmiştir. Bu amaçla açık bir standart olan **Agent Client Protocol (ACP)** kullanılmıştır. ACP sayesinde Amarcode, belirli ajan adlarını ve davranışlarını doğrudan kullanıcı arayüzüne kodlamak yerine ajanların bildirdiği yetenekleri, oturum seçeneklerini ve araç çağrılarını ortak bir protokol üzerinden işleyebilmektedir.

Staj boyunca proje, ilk kullanıcı arayüzü ve daemon prototipinden başlayarak kalıcı depolama, canlı olay akışı, ACP uyumluluğu, güvenli araç çalıştırma, ajan kayıt sistemi ve otomatik daemon yönetimini içeren çok bileşenli bir masaüstü uygulamasına dönüştürülmüştür.

## Kullanılan Teknolojiler

Amarcode farklı sorumluluklar için birden fazla teknoloji kullanan monorepo yapısında geliştirilmiştir:

| Teknoloji | Projedeki kullanım alanı |
|---|---|
| React | Sohbet, ayarlar, ajan seçimi, dosya ağacı ve diff bileşenlerinin geliştirilmesi |
| TypeScript | Ön yüz veri modellerinin ve istemci protokol bağlarının tür güvenli biçimde kullanılması |
| Tauri | React arayüzünün masaüstü uygulamasına dönüştürülmesi ve yerel Rust işlevlerine erişim |
| Rust | Daemon, Tauri yerel katmanı, ortak protokol ve ACP adaptörünün geliştirilmesi |
| Tokio | Eş zamanlı ağ bağlantıları, ajan süreçleri ve uzun süreli görevlerin yönetilmesi |
| SQLite | Sohbet, mesaj, çalışma, olay ve ajan bilgilerinin kalıcı olarak saklanması |
| Jotai | React uygulamasındaki paylaşılan durumun küçük ve test edilebilir atomlara ayrılması |
| Bun | JavaScript çalışma alanının, geliştirme komutlarının ve paket süreçlerinin yönetilmesi |
| Cloudflare Workers | Daemon ve ajan kayıt bilgilerinin servis edilmesi |
| ACP | Masaüstü istemcisi ile farklı kodlama ajanları arasında ortak çalışma modeli |
| JSON-RPC | Daemon ile ACP ajan süreçleri arasındaki yapılandırılmış mesajlaşma |
| GitHub Actions | Platform derlemeleri, kontroller ve masaüstü dağıtım iş akışları |

## Sistem Mimarisinin Geliştirilmesi

Projenin başlangıcında en önemli kararlardan biri kullanıcı arayüzü ile uzun süre çalışan ajan görevlerini birbirinden ayırmak olmuştur. Bir ajan görevi uzun sürebildiği için bütün işlem durumunun React uygulamasında tutulması güvenilir değildir. Masaüstü penceresinin kapanması, bağlantının kesilmesi veya uygulamanın yeniden açılması durumunda görev ve sohbet bilgilerinin kaybolmaması gerekmektedir. Bu nedenle bağımsız çalışan bir **Amarcode daemon** geliştirilmiştir.

![Amarcode sistem mimarisi](https://raw.githubusercontent.com/amar-jay/amarcode/main/assets/diagrams/system-architecture.svg)

*Şekil 1 — Amarcode masaüstü uygulaması, daemon, SQLite, ACP ajanı, kayıt sistemi ve çalışma alanı arasındaki temel ilişkiler.*

React arayüzü kullanıcı etkileşiminden ve bilgilerin görüntülenmesinden sorumludur. Tauri katmanı, ön yüz ile işletim sistemi ve daemon arasında güvenli bir köprü oluşturur. Daemon ise sohbetlerin, mesajların, ajan çalışmalarının ve canlı olayların gerçek sahibidir. Daemon içindeki RPC katmanı TCP üzerinden gelen istekleri alır; servis katmanı iş akışlarını yürütür; saklama katmanı SQLite işlemlerini gerçekleştirir; ACP katmanı ise haricî ajan süreçleriyle standart giriş ve çıkış üzerinden iletişim kurar.

Mimari katmanlar arasında yönlü bağımlılık uygulanmıştır. Saklama katmanı ACP veya TCP ayrıntılarını bilmez, ACP katmanı da sohbet veritabanına doğrudan erişmez. Bu iki alanı birleştiren tek yer servis katmanıdır. Böylece iletişim protokolü, veritabanı şeması veya ajan uygulaması değiştiğinde değişikliğin etkisi daha sınırlı kalmaktadır.

## Kullanıcı Arayüzü Çalışmaları

İlk aşamada kullanıcıya sade ve anlaşılır bir başlangıç deneyimi sunan yeni sohbet ekranı geliştirilmiştir. Kullanıcı çalışma alanını ve ajanı seçtikten sonra çok satırlı bir istem yazabilmektedir. İstem giriş bileşeni boş görevleri engellemekte, işlem devam ederken çalışma durumunu göstermekte ve model ya da oturum seçenekleri için genişletilebilir kontroller sunmaktadır.

Uygulama arayüzü; sohbetlerin bulunduğu sol kenar çubuğu, aktif konuşmanın gösterildiği merkez alan ve çalışma alanı araçlarının açıldığı sağ panel biçiminde düzenlenmiştir. Açık ve koyu tema desteği; metin, arka plan, kenarlık, kod bloğu, diff ve durum göstergelerinde tutarlı biçimde uygulanmıştır.

![Amarcode açık ve koyu tema](https://raw.githubusercontent.com/amar-jay/amarcode/main/assets/amarcode-theme-split.png)

*Şekil 2 — Amarcode masaüstü uygulamasının açık ve koyu tema görünümü.*

Ajan cevaplarının yalnızca düz metin olmadığı dikkate alınarak mesajlar yapılandırılmış parçalara ayrılmıştır. Normal metin, kod blokları, düşünme içeriği, araç çağrıları, test sonuçları ve dosya değişiklikleri farklı bileşenlerle gösterilmektedir. Akış devam ederken yalnızca ilgili mesaj güncellenmekte ve aktif cevap hareketli bir imleçle belirtilmektedir. Kullanıcı önceki bir mesajı incelerken sayfanın zorla en alta kaydırılmaması için kaydırma davranışı da kontrol edilmiştir.

Çalışma alanı dosya ağacı sayesinde kullanıcı proje klasörlerini açıp kapatabilmekte ve kaynak dosyalarını uygulama içinde görüntüleyebilmektedir. Sohbet içinde ajan tarafından verilen göreli dosya yolları etkin çalışma alanına göre çözümlenmekte ve güvenli biçimde yan panelde açılmaktadır. Diff görüntüleyici, eklenen ve silinen satırları ayırmakta; uzun değişiklikleri daraltmaya ve dosyaları sohbet bağlamından ayrılmadan incelemeye imkân vermektedir. Dosya ve sohbet araması ile uzun kullanıcı istemlerini ek olarak saklama özellikleri de geliştirilmiştir.

## İstem, Sohbet ve Canlı Olay Akışı

Kullanıcı istemi gönderdiğinde arayüzde gecikme hissini azaltmak için iyimser güncelleme yaklaşımı uygulanmıştır. Kullanıcı mesajı geçici olarak hemen gösterilmekte, daemon kalıcı kaydı oluşturduktan sonra geçici kimlik gerçek mesaj kimliğiyle eşleştirilmektedir. İstek başarısız olursa kullanıcının yazdığı metnin kaybolmamasına dikkat edilmiştir.

![Kullanıcı isteminin yaşam döngüsü](https://raw.githubusercontent.com/amar-jay/amarcode/main/assets/diagrams/prompt-lifecycle-sequence.svg)

*Şekil 3 — Kullanıcı isteminin arayüz, Tauri, daemon, SQLite ve ACP ajanı arasındaki yaşam döngüsü.*

Daemon ile masaüstü uygulaması arasında iki farklı iletişim biçimi kullanılmıştır. Kısa işlemler TCP JSON-line RPC istek ve yanıtlarıyla yürütülürken, uzun süren görevlerin mesaj ve durum güncellemeleri ayrı bir canlı olay aboneliği üzerinden iletilmektedir. İstemci `subscribe_events` yöntemiyle sohbet, çalışma veya oturum filtresi kullanarak olaylara abone olabilmektedir.

Bağlantı kesildiğinde arayüz bunu açık bir durum olarak göstermekte ve kontrollü biçimde yeniden bağlanmayı denemektedir. Yeniden bağlantı sonrasında yalnızca yeni olaylara güvenilmemekte, kalıcı sohbet durumu tekrar alınarak arayüzle uzlaştırılmaktadır. Böylece kopukluk sırasında üretilen mesajların kaybolması engellenmiştir.

## Eş Zamanlılık ve Veri Tutarlılığı

Aynı sohbet üzerinde birden fazla istemin eş zamanlı başlatılması oturum mesajlarının karışmasına neden olabileceği için sohbet bazlı çalışma kilitleri ve sahiplik kontrolleri uygulanmıştır. Her çalışma benzersiz bir `run_id` değeriyle tanımlanır. Gecikmiş bir olay yalnızca hâlen aktif çalışmaya aitse durumu değiştirebilir. Eski bir çalışma yeni görevi tamamlanmış veya başarısız olarak işaretleyemez.

Bekleyen izin ve kullanıcı girişi istekleri de çalışma kimliğiyle ilişkilendirilmiştir. Bir çalışma bittiğinde ona bağlı bekleyen istekler temizlenir. Yanıt, doğru istek kimliğine sahip olsa bile başka bir çalışmaya aitse kabul edilmez. Bu kontroller özellikle iptal, bağlantı kesilmesi ve hızlı sohbet değişimi durumlarında veri bütünlüğünü korumaktadır.

Kalıcı veri akışında **“önce kaydet, sonra bildir”** ilkesi uygulanmıştır. Ajan tarafından gelen anlamlı bir mesaj veya durum güncellemesi önce SQLite’a yazılır. İşlem başarıyla commit edildikten sonra arayüze `EditorEvent` gönderilir. Veritabanı yazımı başarısız olursa kalıcı olmayan durum kullanıcıya başarılı olarak gösterilmez.

![Önce kaydet olay akışı](https://raw.githubusercontent.com/amar-jay/amarcode/main/assets/diagrams/store-first-event-flow.svg)

*Şekil 4 — Sahiplik doğrulaması, SQLite işlemi ve canlı olay yayını arasındaki veri tutarlılığı akışı.*

SQLite üzerinde sohbet, mesaj, mesaj parçası, çalışma, ajan ve ham ACP olayları saklanmaktadır. Yabancı anahtar kontrolleri ve WAL modu kullanılmış, şema değişiklikleri numaralı migration dosyalarıyla yönetilmiştir. Daemon beklenmedik biçimde kapanırsa bir sonraki başlangıçta yarım kalmış çalışmalar tespit edilerek gerçeğe uygun duruma geçirilir.

## Ortak Protokol ve Tür Güvenliği

Masaüstü uygulaması ile daemon arasındaki istek, yanıt ve olay türleri başlangıçta farklı katmanlarda tekrar tanımlanıyordu. Bu tekrarın uyumsuzluk oluşturmasını engellemek için `amarcode-protocol` adlı ortak Rust crate’i iletişim sözleşmesinin tek kaynağı hâline getirilmiştir. Daemon ve Tauri aynı Rust türlerini kullanmakta, React tarafı için gerekli TypeScript bağları bu türlerden otomatik olarak üretilmektedir.

Protokole açık bir sürüm numarası eklenmiştir. Uygulama daemon’a bağlandığında sağlık ve sürüm kontrolü yaparak desteklenen protokol sürümüyle uyumluluğu doğrular. Uyumsuzluk durumunda normal veri akışına geçilmez ve kullanıcıya uygun daemon güncelleme süreci sunulur. Üretilmiş TypeScript dosyasının Rust tanımlarıyla güncel olduğunu doğrulayan otomatik kontrol de eklenmiştir.

## Daemon Servisi, Kurulum ve Güncelleme

Daemon başlangıçta geliştirme ortamında elle çalıştırılan bir süreçken, staj sürecinde kullanıcı hesabına bağlı bağımsız bir servis hâline getirilmiştir. Kurma, başlatma, durdurma, yeniden başlatma, durum sorgulama ve kaldırma komutları geliştirilmiştir. Tek örnek kilidi sayesinde aynı uygulama dizini ve veritabanı için iki daemon sürecinin eş zamanlı çalışması engellenmektedir.

Masaüstü uygulaması açıldığında daemon’ın kurulu ve çalışır durumda olup olmadığını otomatik olarak denetler. Gerekli sürüm bulunmuyorsa platforma uygun paket kayıt servisinden alınır. Paket geçici konuma indirilir, bütünlük ve sürüm bilgisi doğrulanır, mevcut servis kontrollü biçimde durdurulur ve yeni sürüm atomik olarak yerleştirilir. Yeni daemon sağlık ve protokol kontrolünü geçemezse önceki sürüm geri yüklenir.

![Daemon güncelleme ve geri alma akışı](https://raw.githubusercontent.com/amar-jay/amarcode/main/assets/diagrams/daemon-update-rollback-sequence.svg)

*Şekil 5 — Daemon paketinin alınması, doğrulanması, değiştirilmesi ve hata hâlinde geri yüklenmesi.*

Windows paketleme ve NSIS kaldırıcı kancaları üzerinde de çalışılmıştır. Tam kaldırma sırasında servis kaydı, daemon ikili dosyası ve kullanıcının seçimine bağlı uygulama verileri kontrollü biçimde temizlenmektedir. Linux ve Windows dosya yolları arasındaki farklılıklar ile yüksek DPI pencere boyutlandırması gibi platforma özgü konular da ele alınmıştır.

## ACP Entegrasyonu ve Ajan Oturumları

Daemon içinde geliştirilen ACP istemcisi, kodlama ajanlarını alt süreç olarak başlatmakta ve standart giriş/çıkış üzerinden JSON-RPC mesajlarıyla haberleşmektedir. İstekler benzersiz kimliklerle bekleyen çağrılara bağlanmakta, ajan bildirimleri servis katmanında Amarcode olaylarına dönüştürülmektedir. Metinsel ve sayısal RPC kimlikleri desteklenerek farklı ACP uygulamalarıyla uyumluluk sağlanmıştır.

Her sohbetin aktif ajanı, çalışma kimliği ve ACP oturum kimliği açık biçimde saklanmaktadır. Önceki bir ACP oturumu devam ettirilebiliyorsa resume akışı denenir; buna rağmen kalıcı konuşma geçmişi daemon için yetkili kayıt olarak korunur. Kullanıcı görevi iptal ettiğinde yalnızca arayüz durumu değiştirilmez; gerçek ACP iptal mesajı çalışan sürece iletilir ve ilgili kaynaklar temizlenir.

Ajanların bildirdiği model, mod ve düşünme seçenekleri sabit arayüz kodlarından çıkarılmıştır. Seçim ve boolean türündeki ACP yapılandırmaları genel kontrollerle gösterilir. Seçeneklerin tam anlık görüntüsü SQLite’ta saklanır, ajan bazında hatırlanır ve aktif oturum değişiklikleri canlı olarak ajana iletilir.

## Araç Çağrıları, İzinler ve Güvenlik

ACP adaptörüne `read_file`, `list_directory`, `search_text`, `write_file` ve `run_command` araçları eklenmiştir. Salt okunur işlemler yalnızca etkin çalışma alanında yürütülür. Dosya yazma ve komut çalıştırma işlemleri kodlama moduna ve kullanıcı iznine bağlanmıştır. Bütün yollar kanonikleştirilerek çalışma alanı kökü altında kaldığı doğrulanır; `..` bileşenleri veya sembolik bağlantılarla klasör dışına çıkış reddedilir.

İzin kararları gereğinden geniş tutulmamıştır. Dosya yazma izni hedef yola, komut izni ise çalıştırılabilir dosya, tam argüman listesi ve çalışma dizinine bağlıdır. Oturumluk izinler yalnızca aynı ACP oturumu boyunca hatırlanır. Hedef veya komut değişirse kullanıcıdan tekrar onay istenir.

Komut çalıştırmada çalıştırılabilir dosya ve argümanlar ayrı tutulduğu için örtük kabuk değerlendirmesi yapılmaz. ACP terminal yaşam döngüsü kullanılarak terminal oluşturma, bitişi bekleme, çıktı alma, iptal etme ve kaynağı serbest bırakma adımları uygulanmıştır. Çıktı boyutu sınırlandırılmış ve UTF-8 karakterlerinin ortasında kesilmemesine dikkat edilmiştir. Kullanıcı iptal ettiğinde Unix sistemlerinde alt süreçlerin de sona ermesi için süreç grubu temizliği uygulanmıştır.

![ACP araç izni ve terminal akışı](https://raw.githubusercontent.com/amar-jay/amarcode/main/assets/diagrams/acp-tool-permission-sequence.svg)

*Şekil 6 — Model araç çağrısından kullanıcı iznine ve terminal sonucuna uzanan güvenli işlem sırası.*

## Ajan Registry ve Kurulum Sistemi

Desteklenen ajanların uygulama içinde sabit olarak tanımlanması yerine manifest tabanlı bir ajan registry sistemi entegre edilmiştir. Daemon ilk başlangıçta registry deposunu sığ Git checkout olarak klonlar, sonraki başlangıçlarda `origin/main` dalına hızlı ileri güncelleme uygular. Ağ kullanılamıyorsa son başarılı checkout ile çalışmaya devam edilir.

`agent.json` manifestlerinden ajan kimliği, adı, sürümü ve dağıtım bilgileri okunmaktadır. NPM tabanlı ajanlar `bunx`, Python tabanlı ajanlar `uvx`, ikili dağıtımlar ise işletim sistemi ve mimariye uygun komut ile çalıştırılmaktadır. Bir ajanın katalogda bulunması ve yerel sistemde kullanılabilir olması ayrı durumlar olarak saklanmaktadır. Kurulumdan sonra çalışma zamanı yeniden denetlenerek işlemin gerçekten başarılı olduğu doğrulanmaktadır.

![Ajan registry ve kurulum akışı](https://raw.githubusercontent.com/amar-jay/amarcode/main/assets/diagrams/agent-registry-install-flow.svg)

*Şekil 7 — Registry eşitleme, manifest işleme, kullanılabilirlik denetimi ve ajan kurulum adımları.*

Kimlik doğrulaması gerektiren ajanlar bu gereksinimi yetenekleriyle bildirmektedir. Daemon aktif oturum veya kısa ömürlü deneme süreci üzerinden ACP kimlik doğrulama yöntemini başlatır. Gerçek erişim bilgileri sohbet kayıtlarına ya da kullanıcı arayüzünün genel durumuna yazılmaz.

## Test, Kalite ve Platform Doğrulaması

Geliştirilen özellikler yalnızca arayüz üzerinden elle denenmemiş, farklı katmanlarda otomatik testlerle doğrulanmıştır. React durum testleri canlı mesaj, eski çalışma olayı, sohbet geçişi, izin modu ve yapılandırma anlık görüntüsü gibi davranışları kapsamaktadır. ACP transcript testleri oturum oluşturma, akışlı mesaj kimliği, iptal, araç çağrısı ve terminal mesaj sırasını kontrol etmektedir. Daemon dikey dilim testleri ise bir istemin RPC katmanından SQLite’a, ACP sürecine ve canlı olaya kadar ilerlemesini sınamaktadır.

Rust türlerinden oluşturulan TypeScript protokolünün güncel kalması otomatik testle denetlenmektedir. Lint, biçimlendirme, TypeScript kontrolü, Rust testleri ve Clippy kontrolleri proje komutlarına eklenmiştir. GitHub Actions iş akışlarında Rust çalışma alanı ve Bun paket yapısına uygun düzenlemeler yapılmış, özellikle Windows dosya yolları ve paketleme sorunları giderilmiştir.

Testler için üretim ajanlarına veya dış registry bağlantısına bağımlı olmayan sahte ACP ajanları ve test manifestleri kullanılmıştır. Bu sayede yapılandırma seçenekleri, izin istekleri, bağlantı kesilmesi ve hata durumları tekrar üretilebilir girdilerle doğrulanabilmiştir.

## Çalışmaların Genel Sonucu

Staj süresince Amarcode, temel bir sohbet ve daemon prototipinden farklı ACP ajanlarını destekleyen kapsamlı bir masaüstü geliştirme aracına dönüştürülmüştür. Kullanıcı bir proje açabilmekte, kayıt sisteminden ajan seçebilmekte, gerekli ajanı kurabilmekte, model ve oturum seçeneklerini belirleyebilmekte ve görev başlatabilmektedir. Görev sırasında mesajlar canlı olarak gösterilmekte, dosyalar ve değişiklikler incelenebilmekte, araç çağrıları takip edilebilmekte ve riskli işlemler kullanıcı onayı gerektirmektedir.

Bağımsız daemon ve SQLite kalıcılığı sayesinde sohbetler ile görev durumu masaüstü penceresinden ayrılmıştır. Ortak protokol ve otomatik tür üretimi istemci ile daemon arasındaki uyumluluğu güçlendirmiştir. Sahiplik kontrolleri, önce kaydetme ilkesi, güvenli yol çözümleme ve dar kapsamlı izinler sistemin güvenilirliğini artırmıştır. Registry tabanlı ajan keşfi ve dinamik yapılandırma ise yeni ajanların uygulama koduna daha az bağımlı biçimde eklenebilmesini sağlamıştır.

Bu çalışmalar sırasında masaüstü uygulama geliştirme, Rust eş zamanlılığı, olay tabanlı mimari, protokol tasarımı, kalıcı veri yönetimi, süreç yaşam döngüsü, güvenlik sınırları, test geliştirme ve çapraz platform dağıtım konularında uygulamalı deneyim kazandım.

## İlgili Belgeler

- [Amarcode proje deposu](https://github.com/amar-jay/amarcode)
- [Amarcode README](https://github.com/amar-jay/amarcode/blob/main/README.md)
- [Daemon teknik belgesi](https://github.com/amar-jay/amarcode/blob/main/crates/daemon/README.md)
- [ACP adaptörü teknik belgesi](https://github.com/amar-jay/amarcode/blob/main/crates/amarcode-acp/README.md)
- [Günlük staj raporları](https://github.com/amar-jay/amarcode/tree/main/reports)
- [Toplu kaynakça](https://github.com/amar-jay/amarcode/blob/main/reports/KAYNAKCA.md)
- [Ekler](https://github.com/amar-jay/amarcode/blob/main/reports/EKLER.md)
- [Agent Client Protocol](https://agentclientprotocol.com/)

