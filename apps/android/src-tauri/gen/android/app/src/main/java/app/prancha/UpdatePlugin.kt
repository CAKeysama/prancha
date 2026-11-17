package app.prancha

import android.app.Activity
import android.content.Intent
import androidx.core.content.FileProvider
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File

/** Atualização pelo APK das releases: o Rust baixa e confere o arquivo (crates/prancha-app/src/commands.rs), e o instalador do Android pede a confirmação. */
@TauriPlugin
class UpdatePlugin(private val activity: Activity) : Plugin(activity) {
  // Na pasta de cache, que o FileProvider do app já entrega (res/xml/file_paths.xml). Um nome fixo: o próximo download substitui o anterior.
  private fun apk(): File = File(File(activity.cacheDir, "atualizacao"), "Prancha.apk")

  @Command
  fun updateFile(invoke: Invoke) {
    val file = apk()
    val dir = file.parentFile
    if (dir == null || !(dir.isDirectory || dir.mkdirs())) {
      invoke.reject("Não foi possível criar a pasta da atualização.")
      return
    }
    invoke.resolve(JSObject().put("path", file.absolutePath))
  }

  @Command
  fun installApk(invoke: Invoke) {
    val file = apk()
    if (!file.isFile) {
      invoke.reject("O APK baixado não está mais na pasta da atualização.")
      return
    }
    try {
      val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", file)
      // Sem a permissão de instalar apps desta fonte, o próprio Android leva a pessoa à configuração e volta.
      val intent = Intent(Intent.ACTION_VIEW)
        .setDataAndType(uri, "application/vnd.android.package-archive")
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
      activity.startActivity(intent)
      invoke.resolve()
    } catch (e: Exception) {
      invoke.reject("Não foi possível abrir o instalador do Android: $e")
    }
  }
}
