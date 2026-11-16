package app.prancha

import android.app.Activity
import android.content.Intent
import android.util.Log
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class DownloadingArgs {
  var on: Boolean = false
}

@InvokeArg
class ScheduleArgs {
  lateinit var dataDir: String
}

/** O que roda com o app fora da tela: o serviço de downloads e a verificação de capítulos novos. O Rust chama (crates/prancha-app/src/android.rs). */
@TauriPlugin
class BackgroundPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun setDownloading(invoke: Invoke) {
    val on = invoke.parseArgs(DownloadingArgs::class.java).on
    val intent = Intent(activity, DownloadService::class.java)
    try {
      if (on) ContextCompat.startForegroundService(activity, intent) else activity.stopService(intent)
    } catch (e: Exception) {
      // Android 12 ou mais novo não deixa começar com o app fora da tela: a fila segue enquanto o processo viver.
      Log.w("Prancha", "serviço de downloads: $e")
    }
    invoke.resolve()
  }

  @Command
  fun scheduleFollowCheck(invoke: Invoke) {
    val dataDir = invoke.parseArgs(ScheduleArgs::class.java).dataDir
    try {
      FollowWorker.schedule(activity.applicationContext, dataDir)
      invoke.resolve()
    } catch (e: Exception) {
      invoke.reject("Não foi possível agendar a verificação: $e")
    }
  }
}
