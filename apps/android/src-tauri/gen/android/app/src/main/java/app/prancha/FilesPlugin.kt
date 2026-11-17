package app.prancha

import android.app.Activity
import android.content.Intent
import android.database.Cursor
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import app.tauri.Logger
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.FileNotFoundException
import java.util.concurrent.Executors

@InvokeArg
class UriArgs {
  lateinit var uri: String
}

@InvokeArg
class WatchArgs {
  lateinit var channel: Channel
}

/**
 * Arquivos escolhidos pela pessoa (o seletor do sistema) e lidos por endereço content://. O Rust chama estes
 * comandos (crates/prancha-app/src/android.rs) de threads de trabalho; as consultas ao sistema rodam aqui numa
 * thread própria, para não travar a tela.
 */
@TauriPlugin
class FilesPlugin(private val activity: Activity) : Plugin(activity) {
  private val worker = Executors.newCachedThreadPool()
  private var opened: Channel? = null
  private val pending = mutableListOf<String>()

  override fun load(webView: WebView) {
    received(activity.intent)
  }

  override fun onNewIntent(intent: Intent) {
    received(intent)
  }

  // ---------------------------------------------------------------------------------------------
  // Seletor
  // ---------------------------------------------------------------------------------------------

  @Command
  fun pickFiles(invoke: Invoke) {
    val intent = Intent(Intent.ACTION_OPEN_DOCUMENT)
      .addCategory(Intent.CATEGORY_OPENABLE)
      .setType("*/*")
      .putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
    startActivityForResult(invoke, intent, "pickedFiles")
  }

  @ActivityCallback
  fun pickedFiles(invoke: Invoke, result: ActivityResult) {
    val data = result.data
    if (result.resultCode != Activity.RESULT_OK || data == null) {
      invoke.resolve(JSObject())
      return
    }
    val uris = mutableListOf<Uri>()
    val clip = data.clipData
    if (clip != null) {
      for (i in 0 until clip.itemCount) uris.add(clip.getItemAt(i).uri)
    } else {
      data.data?.let { uris.add(it) }
    }
    uris.forEach { keepAccess(it) }
    invoke.resolve(JSObject().put("uris", JSArray.from(uris.map { it.toString() }.toTypedArray())))
  }

  @Command
  fun pickFolder(invoke: Invoke) {
    startActivityForResult(invoke, Intent(Intent.ACTION_OPEN_DOCUMENT_TREE), "pickedFolder")
  }

  @ActivityCallback
  fun pickedFolder(invoke: Invoke, result: ActivityResult) {
    val tree = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || tree == null) {
      invoke.resolve(JSObject())
      return
    }
    keepAccess(tree)
    // A pasta em si, como documento: é dela que saem a listagem e os filhos.
    val folder = DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
    invoke.resolve(JSObject().put("uris", JSArray.from(arrayOf(folder.toString()))))
  }

  /** A permissão de leitura vale depois de reiniciar o celular: a biblioteca guarda o endereço. */
  private fun keepAccess(uri: Uri) {
    try {
      activity.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
    } catch (e: SecurityException) {
      Logger.error("permissão persistente recusada para $uri: ${e.message}")
    }
  }

  // ---------------------------------------------------------------------------------------------
  // Leitura
  // ---------------------------------------------------------------------------------------------

  @Command
  fun stat(invoke: Invoke) {
    val uri = Uri.parse(invoke.parseArgs(UriArgs::class.java).uri)
    background(invoke) {
      val document = DocumentsContract.isDocumentUri(activity, uri)
      val columns = if (document) {
        arrayOf(
          DocumentsContract.Document.COLUMN_DISPLAY_NAME,
          DocumentsContract.Document.COLUMN_MIME_TYPE,
          DocumentsContract.Document.COLUMN_SIZE,
          DocumentsContract.Document.COLUMN_LAST_MODIFIED,
        )
      } else {
        arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE)
      }
      query(uri, columns).use { c ->
        if (c == null || !c.moveToFirst()) throw FileNotFoundException("$uri não existe mais")
        val out = JSObject()
          .put("name", c.getString(0) ?: uri.lastPathSegment ?: "")
          .put("isDir", document && c.getString(1) == DocumentsContract.Document.MIME_TYPE_DIR)
          .put("size", longOrZero(c, if (document) 2 else 1))
        if (document && !c.isNull(3)) out.put("modified", c.getLong(3))
        out
      }
    }
  }

  @Command
  fun children(invoke: Invoke) {
    val uri = Uri.parse(invoke.parseArgs(UriArgs::class.java).uri)
    background(invoke) {
      val list = DocumentsContract.buildChildDocumentsUriUsingTree(uri, DocumentsContract.getDocumentId(uri))
      val columns = arrayOf(
        DocumentsContract.Document.COLUMN_DOCUMENT_ID,
        DocumentsContract.Document.COLUMN_DISPLAY_NAME,
        DocumentsContract.Document.COLUMN_MIME_TYPE,
        DocumentsContract.Document.COLUMN_SIZE,
      )
      val items = JSArray()
      query(list, columns).use { c ->
        if (c == null) throw FileNotFoundException("$uri não existe mais")
        while (c.moveToNext()) {
          val child = DocumentsContract.buildDocumentUriUsingTree(uri, c.getString(0))
          items.put(
            JSObject()
              .put("uri", child.toString())
              .put("name", c.getString(1) ?: "")
              .put("isDir", c.getString(2) == DocumentsContract.Document.MIME_TYPE_DIR)
              .put("size", longOrZero(c, 3))
          )
        }
      }
      JSObject().put("items", items)
    }
  }

  /** Entrega o descritor ao Rust, que passa a ser o dono dele (fecha junto com o `File`). */
  @Command
  fun open(invoke: Invoke) {
    val uri = Uri.parse(invoke.parseArgs(UriArgs::class.java).uri)
    background(invoke) {
      val pfd = activity.contentResolver.openFileDescriptor(uri, "r") ?: throw FileNotFoundException("$uri não abriu")
      JSObject().put("fd", pfd.detachFd())
    }
  }

  private fun query(uri: Uri, columns: Array<String>): Cursor? = activity.contentResolver.query(uri, columns, null, null, null)

  private fun longOrZero(c: Cursor, index: Int): Long = if (c.isNull(index)) 0 else c.getLong(index)

  /** Documento apagado, permissão revogada ou endereço que o sistema não reconhece: para o Rust, o arquivo sumiu. */
  private fun background(invoke: Invoke, work: () -> JSObject) {
    worker.execute {
      try {
        invoke.resolve(work())
      } catch (e: FileNotFoundException) {
        invoke.reject(e.message, "not_found")
      } catch (e: SecurityException) {
        invoke.reject(e.message, "not_found")
      } catch (e: IllegalArgumentException) {
        invoke.reject(e.message, "not_found")
      } catch (e: Exception) {
        invoke.reject(e.message ?: e.toString())
      }
    }
  }

  // ---------------------------------------------------------------------------------------------
  // Abrir pelo gerenciador de arquivos
  // ---------------------------------------------------------------------------------------------

  /** O Rust chama uma vez, na abertura: cada arquivo aberto de fora chega por este canal. */
  @Command
  fun watchOpened(invoke: Invoke) {
    val channel = invoke.parseArgs(WatchArgs::class.java).channel
    val waiting = synchronized(this) {
      opened = channel
      pending.toList().also { pending.clear() }
    }
    waiting.forEach { channel.send(JSObject().put("path", it)) }
    invoke.resolve()
  }

  private fun received(intent: Intent?) {
    if (intent?.action != Intent.ACTION_VIEW) return
    val uri = intent.data ?: return
    val flags = intent.flags
    // Se a Activity for recriada (girar a tela), o mesmo pedido não abre a HQ de novo.
    intent.action = Intent.ACTION_MAIN
    worker.execute {
      val path = try {
        readable(uri, flags)
      } catch (e: Exception) {
        Logger.error("não foi possível abrir $uri: ${e.message}")
        return@execute
      }
      val channel = synchronized(this) { opened ?: run { pending.add(path); null } }
      channel?.send(JSObject().put("path", path))
    }
  }

  /**
   * Um endereço que continua valendo depois. Quem abriu o arquivo pode dar acesso só até o app fechar; nesse
   * caso a HQ é copiada para a pasta do app (Recebidos), senão sumiria da biblioteca na próxima abertura.
   */
  private fun readable(uri: Uri, flags: Int): String {
    if (uri.scheme == "file") return uri.path ?: throw FileNotFoundException("$uri sem caminho")
    if (flags and Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION != 0) {
      try {
        activity.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        return uri.toString()
      } catch (e: SecurityException) {
        Logger.error("permissão persistente recusada para $uri: ${e.message}")
      }
    }
    var name = "quadrinho"
    var size = -1L
    query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE)).use { c ->
      if (c != null && c.moveToFirst()) {
        c.getString(0)?.let { name = File(it).name }
        if (!c.isNull(1)) size = c.getLong(1)
      }
    }
    val dir = File(activity.filesDir, "Recebidos").apply { mkdirs() }
    var target = File(dir, name)
    // O mesmo arquivo aberto de novo reaproveita a cópia; outro com o mesmo nome ganha um número.
    var n = 2
    while (target.exists() && target.length() != size) {
      target = File(dir, "${name.substringBeforeLast('.')} ($n).${name.substringAfterLast('.', "")}".removeSuffix("."))
      n++
    }
    if (!target.exists()) {
      val partial = File(dir, ".${target.name}.parcial")
      activity.contentResolver.openInputStream(uri).use { input ->
        if (input == null) throw FileNotFoundException("$uri não abriu")
        partial.outputStream().use { input.copyTo(it) }
      }
      partial.renameTo(target)
    }
    return target.absolutePath
  }
}
