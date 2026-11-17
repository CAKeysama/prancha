package app.prancha

import android.app.Activity
import android.app.Application
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import android.webkit.CookieManager
import android.webkit.WebSettings
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import dalvik.system.PathClassLoader
import eu.kanade.tachiyomi.network.GET
import eu.kanade.tachiyomi.network.JavaScriptEngine
import eu.kanade.tachiyomi.network.NetworkHelper
import eu.kanade.tachiyomi.network.awaitSuccess
import eu.kanade.tachiyomi.source.CatalogueSource
import eu.kanade.tachiyomi.source.Source
import eu.kanade.tachiyomi.source.SourceFactory
import eu.kanade.tachiyomi.source.model.Page
import eu.kanade.tachiyomi.source.model.SChapter
import eu.kanade.tachiyomi.source.model.SManga
import eu.kanade.tachiyomi.source.online.HttpSource
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.protobuf.ProtoBuf
import okhttp3.Response
import uy.kohesive.injekt.Injekt
import uy.kohesive.injekt.api.addSingleton
import uy.kohesive.injekt.api.addSingletonFactory
import java.io.File
import java.io.IOException
import java.security.MessageDigest
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors

@InvokeArg
class MihonInstallArgs {
  lateinit var apk: String
  lateinit var fingerprint: String
  lateinit var packageName: String
}

@InvokeArg
class MihonPackageArgs {
  lateinit var packageName: String
}

@InvokeArg
class MihonBrowseArgs {
  lateinit var source: String
  lateinit var kind: String
  var page: Int = 1
  var query: String? = null
}

@InvokeArg
class MihonUrlArgs {
  lateinit var source: String
  lateinit var url: String
}

@InvokeArg
class MihonAddressArgs {
  lateinit var source: String
  var manga: String? = null
  var chapter: String? = null
}

@InvokeArg
class MihonPageArg {
  var index: Int = 0
  var url: String = ""
  var imageUrl: String? = null
}

@InvokeArg
class MihonImageArgs {
  lateinit var source: String
  var cover: String? = null
  var page: MihonPageArg? = null
  lateinit var file: String
  var maxBytes: Long = 0
}

@InvokeArg
class MihonVerifyArgs {
  lateinit var url: String
}

/**
 * As extensões do Mihon dentro do Prancha. Elas são APKs do Android: o pacote fica na pasta do app, é carregado por um
 * class loader próprio e roda sobre a mesma API que o Mihon dá a elas (o código em `eu/kanade/tachiyomi`, adaptado do
 * Mihon). O núcleo em Rust (crates/prancha-core/src/extensions/mihon.rs) baixa o APK e faz as perguntas; aqui ficam o
 * carregamento e as respostas. Cada comando roda fora da thread principal, porque as extensões falam com a rede.
 */
@TauriPlugin
class MihonPlugin(private val activity: Activity) : Plugin(activity) {
  private val work = Executors.newCachedThreadPool()

  init {
    MihonExtensions.init(activity.application)
  }

  private fun run(invoke: Invoke, block: () -> Any?) {
    work.execute {
      try {
        when (val result = block()) {
          is JSObject -> invoke.resolve(result)
          // Listas vão dentro de um objeto, que é o que a ponte do Tauri serializa como JSON.
          is JSArray -> invoke.resolve(JSObject().put("items", result))
          else -> invoke.resolve()
        }
      } catch (e: Throwable) {
        Log.w(TAG, "falha na extensão", e)
        invoke.reject(message(e))
      }
    }
  }

  @Command
  fun install(invoke: Invoke) {
    val args = invoke.parseArgs(MihonInstallArgs::class.java)
    run(invoke) { MihonExtensions.install(File(args.apk), args.fingerprint, args.packageName) }
  }

  @Command
  fun uninstall(invoke: Invoke) {
    val args = invoke.parseArgs(MihonPackageArgs::class.java)
    run(invoke) { MihonExtensions.uninstall(args.packageName) }
  }

  @Command
  fun browse(invoke: Invoke) {
    val args = invoke.parseArgs(MihonBrowseArgs::class.java)
    run(invoke) { MihonExtensions.browse(args.source, args.kind, args.page, args.query.orEmpty()) }
  }

  @Command
  fun details(invoke: Invoke) {
    val args = invoke.parseArgs(MihonUrlArgs::class.java)
    run(invoke) { MihonExtensions.details(args.source, args.url) }
  }

  @Command
  fun chapters(invoke: Invoke) {
    val args = invoke.parseArgs(MihonUrlArgs::class.java)
    run(invoke) { MihonExtensions.chapters(args.source, args.url) }
  }

  @Command
  fun pages(invoke: Invoke) {
    val args = invoke.parseArgs(MihonUrlArgs::class.java)
    run(invoke) { MihonExtensions.pages(args.source, args.url) }
  }

  @Command
  fun address(invoke: Invoke) {
    val args = invoke.parseArgs(MihonAddressArgs::class.java)
    run(invoke) { MihonExtensions.address(args.source, args.manga, args.chapter) }
  }

  @Command
  fun image(invoke: Invoke) {
    val args = invoke.parseArgs(MihonImageArgs::class.java)
    run(invoke) { MihonExtensions.image(args) }
  }

  /** Espera a pessoa sair da tela do site. A espera fica numa thread de trabalho, nunca na principal. */
  @Command
  fun verify(invoke: Invoke) {
    val args = invoke.parseArgs(MihonVerifyArgs::class.java)
    run(invoke) {
      val done = CountDownLatch(1)
      val ticket = VerifyActivity.waitFor(done)
      activity.runOnUiThread {
        activity.startActivity(Intent(activity, VerifyActivity::class.java).putExtra(VerifyActivity.URL, args.url).putExtra(VerifyActivity.TICKET, ticket))
      }
      done.await()
      JSObject().put("cleared", MihonExtensions.hasClearance(args.url))
    }
  }

  companion object {
    private const val TAG = "PranchaMihon"

    /** A frase que chega ao núcleo: a da exceção mais funda, com o nome da classe só quando ela não tem mensagem. */
    fun message(error: Throwable): String {
      var root = error
      while (root.cause != null && root.cause !== root && (root.message.isNullOrBlank() || root is IOException && root.cause is IOException)) {
        root = root.cause!!
      }
      return root.message?.takeIf { it.isNotBlank() } ?: root.javaClass.name
    }
  }
}

/** As extensões instaladas, carregadas uma vez por processo, e as fontes delas pelo número que o Mihon dá a cada uma. */
object MihonExtensions {
  private const val TAG = "PranchaMihon"
  private const val FEATURE = "tachiyomi.extension"
  private const val SOURCE_CLASS = "tachiyomi.extension.class"
  private const val NSFW = "tachiyomi.extension.nsfw"
  private const val CONTENT_WARNING = "tachiyomix.contentWarning"
  private const val EXTENSION_LIB = "tachiyomix.extensionLib"
  private val SUPPORTED_LIB_VERSIONS = listOf(1.4, 1.5, 1.6)

  @Suppress("DEPRECATION")
  private val FLAGS = PackageManager.GET_CONFIGURATIONS or PackageManager.GET_META_DATA or PackageManager.GET_SIGNATURES or
    (if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) PackageManager.GET_SIGNING_CERTIFICATES else 0)

  private lateinit var app: Application
  private val sources = ConcurrentHashMap<String, CatalogueSource>()
  private val byPackage = ConcurrentHashMap<String, List<String>>()
  @Volatile private var loaded = false

  /** O `memo` que as extensões novas guardam na obra e no capítulo, e esperam receber de volta nas chamadas seguintes. */
  private val memos by lazy { app.getSharedPreferences("mihon_memo", Context.MODE_PRIVATE) }
  private val json = Json { ignoreUnknownKeys = true; explicitNulls = false }

  @Synchronized
  fun init(application: Application) {
    if (::app.isInitialized) return
    app = application
    // O que as extensões pedem ao Injekt, como no Mihon.
    Injekt.addSingleton<Application>(application)
    Injekt.addSingleton<Context>(application)
    Injekt.addSingleton(json)
    Injekt.addSingleton(ProtoBuf)
    Injekt.addSingletonFactory { NetworkHelper(application) }
    Injekt.addSingletonFactory { JavaScriptEngine(application) }
  }

  private fun folder() = File(app.filesDir, "mihon").apply { mkdirs() }

  private fun file(packageName: String) = File(folder(), "$packageName.ext")

  @Synchronized
  private fun ensureLoaded() {
    if (loaded) return
    // O mesmo navegador que a tela de verificação usa: a liberação de um site vale só para ele.
    runCatching { NetworkHelper.USER_AGENT = WebSettings.getDefaultUserAgent(app) }
    folder().listFiles { f -> f.isFile && f.name.endsWith(".ext") }?.forEach { f ->
      try {
        load(f, null)
      } catch (e: Throwable) {
        Log.w(TAG, "extensão não carregou: ${f.name}", e)
      }
    }
    loaded = true
  }

  private fun signatures(info: PackageInfo): List<String> {
    val signatures = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      val signing = info.signingInfo ?: return emptyList()
      if (signing.hasMultipleSigners()) signing.apkContentsSigners else signing.signingCertificateHistory
    } else {
      @Suppress("DEPRECATION") info.signatures
    }
    return signatures.orEmpty().map { MessageDigest.getInstance("SHA-256").digest(it.toByteArray()).joinToString("") { b -> "%02x".format(b) } }
  }

  private fun archive(path: File): PackageInfo {
    val info = app.packageManager.getPackageArchiveInfo(path.absolutePath, FLAGS) ?: throw IOException("O arquivo baixado não é uma extensão do Mihon.")
    info.applicationInfo?.apply {
      sourceDir = path.absolutePath
      publicSourceDir = path.absolutePath
    }
    return info
  }

  private fun contentWarning(info: PackageInfo): String {
    val meta = info.applicationInfo?.metaData ?: return "SAFE"
    return when {
      meta.containsKey(CONTENT_WARNING) -> when (meta.getInt(CONTENT_WARNING)) { 1 -> "MIXED"; 2 -> "NSFW"; else -> "SAFE" }
      meta.getInt(NSFW) == 1 -> "NSFW"
      else -> "SAFE"
    }
  }

  /** Confere o pacote antes de ele rodar qualquer código: extensão do Mihon, numa versão da API que o Prancha tem, com o nome e a assinatura esperados. */
  private fun check(info: PackageInfo, packageName: String?, fingerprint: String?) {
    if (info.reqFeatures.orEmpty().none { it.name == FEATURE }) throw IOException("O pacote não é uma extensão do Mihon.")
    if (packageName != null && info.packageName != packageName) throw IOException("O pacote baixado não é o que o repositório anunciou.")
    val meta = info.applicationInfo?.metaData ?: throw IOException("A extensão está incompleta.")
    val version = info.versionName.orEmpty()
    val lib = meta.getFloat(EXTENSION_LIB).takeUnless { it == 0f }?.toString()?.toDouble() ?: version.substringBeforeLast('.').toDoubleOrNull()
    if (lib == null || lib !in SUPPORTED_LIB_VERSIONS) throw IOException("Esta extensão usa a versão $lib da API do Mihon, que o Prancha ainda não tem.")
    val signed = signatures(info)
    if (signed.isEmpty()) throw IOException("A extensão não está assinada.")
    if (fingerprint != null && fingerprint.lowercase() !in signed) throw IOException("A extensão não foi assinada pela chave do repositório.")
  }

  private fun load(path: File, fingerprint: String?): JSObject {
    val info = archive(path)
    check(info, path.name.removeSuffix(".ext"), fingerprint)
    val loader = PathClassLoader(path.absolutePath, null, app.classLoader)
    val classes = info.applicationInfo?.metaData?.getString(SOURCE_CLASS)?.takeIf { it.isNotBlank() } ?: throw IOException("A extensão não diz quais são as fontes dela.")
    val found = classes.split(";").map { it.trim() }.map { if (it.startsWith(".")) info.packageName + it else it }.flatMap {
      when (val obj = Class.forName(it, false, loader).getDeclaredConstructor().newInstance()) {
        is SourceFactory -> obj.createSources()
        is Source -> listOf(obj)
        else -> throw IOException("A extensão tem uma fonte de um tipo desconhecido.")
      }
    }.filterIsInstance<CatalogueSource>()
    byPackage.remove(info.packageName)?.forEach { sources.remove(it) }
    found.forEach { sources[it.id.toString()] = it }
    byPackage[info.packageName] = found.map { it.id.toString() }
    val list = JSArray()
    found.forEach { list.put(JSObject().put("id", it.id.toString()).put("name", it.name).put("lang", it.lang).put("supportsLatest", it.supportsLatest)) }
    return JSObject().put("contentWarning", contentWarning(info)).put("sources", list)
  }

  @Synchronized
  fun install(apk: File, fingerprint: String, packageName: String): JSObject {
    ensureLoaded()
    // Conferido antes de copiar: um pacote recusado nem chega à pasta das extensões.
    check(archive(apk), packageName, fingerprint)
    val target = file(packageName)
    val staged = File(folder(), "$packageName.ext.parte")
    try {
      staged.delete()
      apk.copyTo(staged, overwrite = true)
      // O Android 14 só carrega código de arquivos que ninguém pode alterar.
      staged.setReadOnly()
      target.delete()
      if (!staged.renameTo(target)) throw IOException("Não foi possível guardar a extensão.")
    } finally {
      staged.delete()
    }
    return load(target, fingerprint)
  }

  @Synchronized
  fun uninstall(packageName: String): JSObject? {
    byPackage.remove(packageName)?.forEach { sources.remove(it) }
    file(packageName).delete()
    return null
  }

  private fun source(id: String): CatalogueSource {
    ensureLoaded()
    return sources[id] ?: throw IOException("Esta fonte não está instalada no celular. Instale a extensão de novo.")
  }

  private fun memoKey(source: String, kind: String, url: String) = "$source|$kind|$url"

  private fun remember(source: String, kind: String, url: String, memo: JsonObject?) {
    if (memo != null && memo.isNotEmpty()) memos.edit().putString(memoKey(source, kind, url), memo.toString()).apply()
  }

  private fun recall(source: String, kind: String, url: String): JsonObject? =
    memos.getString(memoKey(source, kind, url), null)?.let { runCatching { json.decodeFromString(JsonObject.serializer(), it) }.getOrNull() }

  private fun manga(source: String, url: String) = SManga.create().also { m ->
    m.url = url
    recall(source, "m", url)?.let { m.memo = it }
  }

  private fun chapter(source: String, url: String) = SChapter.create().also { c ->
    c.url = url
    recall(source, "c", url)?.let { c.memo = it }
  }

  private fun <T> safe(block: () -> T): T? = try { block() } catch (_: UninitializedPropertyAccessException) { null }

  private fun genres(manga: SManga): JSArray {
    val list = JSArray()
    manga.genre.orEmpty().split(",").map { it.trim() }.filter { it.isNotEmpty() }.forEach { list.put(it) }
    return list
  }

  fun browse(id: String, kind: String, page: Int, query: String): JSObject {
    val source = source(id)
    val found = runBlocking {
      when (kind) {
        "latest" -> source.getLatestUpdates(page)
        "search" -> source.getSearchManga(page, query, source.getFilterList())
        else -> source.getPopularManga(page)
      }
    }
    val list = JSArray()
    found.mangas.forEach { m ->
      val url = safe { m.url } ?: return@forEach
      remember(id, "m", url, m.memo)
      list.put(JSObject().put("url", url).put("title", safe { m.title }.orEmpty()).put("thumbnailUrl", m.thumbnail_url).put("genre", genres(m)))
    }
    return JSObject().put("mangas", list).put("hasNextPage", found.hasNextPage)
  }

  fun details(id: String, url: String): JSObject {
    val source = source(id)
    val m = runBlocking { source.getMangaUpdate(manga(id, url), emptyList(), fetchDetails = true, fetchChapters = false) }.manga
    remember(id, "m", url, m.memo)
    return JSObject()
      .put("title", safe { m.title }.orEmpty())
      .put("author", m.author)
      .put("artist", m.artist)
      .put("description", m.description)
      .put("status", m.status)
      .put("genre", genres(m))
      .put("thumbnailUrl", m.thumbnail_url)
  }

  fun chapters(id: String, url: String): JSArray {
    val source = source(id)
    val list = runBlocking { source.getMangaUpdate(manga(id, url), emptyList(), fetchDetails = false, fetchChapters = true) }.chapters
    val out = JSArray()
    list.forEach { c ->
      val chapterUrl = safe { c.url } ?: return@forEach
      remember(id, "c", chapterUrl, c.memo)
      out.put(JSObject().put("url", chapterUrl).put("name", safe { c.name }.orEmpty()).put("chapterNumber", c.chapter_number.toDouble()).put("dateUpload", c.date_upload))
    }
    return out
  }

  fun pages(id: String, url: String): JSArray {
    val source = source(id)
    val list = runBlocking { source.getPageList(chapter(id, url)) }
    val out = JSArray()
    list.forEach { p -> out.put(JSObject().put("index", p.index).put("url", p.url).put("imageUrl", p.imageUrl)) }
    return out
  }

  fun address(id: String, mangaUrl: String?, chapterUrl: String?): JSObject {
    val source = source(id) as? HttpSource ?: return JSObject().put("url", "")
    val url = when {
      chapterUrl != null -> runCatching { source.getChapterUrl(chapter(id, chapterUrl)) }.getOrNull()
      mangaUrl != null -> runCatching { source.getMangaUrl(manga(id, mangaUrl)) }.getOrNull()
      else -> null
    } ?: source.baseUrl
    return JSObject().put("url", url)
  }

  /** Grava a imagem no arquivo que o núcleo indicou, parando no limite de tamanho dele. */
  fun image(args: MihonImageArgs): JSObject {
    val source = source(args.source) as? HttpSource ?: throw IOException("Esta fonte não busca imagens pela internet.")
    val response: Response = runBlocking {
      val cover = args.cover
      if (cover != null) {
        source.client.newCall(GET(cover, source.headers)).awaitSuccess()
      } else {
        val arg = args.page ?: throw IOException("Página inválida.")
        val page = Page(arg.index, arg.url, arg.imageUrl)
        if (page.imageUrl.isNullOrEmpty()) page.imageUrl = source.getImageUrl(page)
        source.getImage(page)
      }
    }
    response.use {
      val body = it.body
      var total = 0L
      var tooLarge = false
      File(args.file).outputStream().use { out ->
        body.byteStream().use { input ->
          val buffer = ByteArray(64 * 1024)
          while (true) {
            val read = input.read(buffer)
            if (read < 0) break
            total += read
            if (args.maxBytes in 1 until total) {
              tooLarge = true
              break
            }
            out.write(buffer, 0, read)
          }
        }
      }
      return JSObject()
        .put("status", it.code)
        .put("url", it.request.url.toString())
        .put("contentType", body.contentType()?.toString())
        .put("tooLarge", tooLarge)
    }
  }

  fun hasClearance(url: String): Boolean = CookieManager.getInstance().getCookie(url).orEmpty().split(";").any { it.trim().startsWith("cf_clearance=") }
}
