package app.prancha

import android.Manifest
import android.annotation.SuppressLint
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.Worker
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import java.util.concurrent.TimeUnit
import org.json.JSONArray

/** A ponte com o núcleo em Rust (apps/android/src-tauri/src/lib.rs). */
object PranchaNative {
  init {
    System.loadLibrary("prancha_android_lib")
  }

  /** Verifica as obras acompanhadas e devolve, em JSON, os avisos que o app mostraria: `[{id, title, body}]`. */
  @JvmStatic external fun checkFollowing(dataDir: String, context: Context): String
}

/**
 * Capítulos novos com o app fechado: o WorkManager roda isto a cada 6 horas, com internet e bateria boa. A
 * verificação é a mesma de Explorar; os avisos entram na central do app e, com a permissão, viram notificações.
 */
class FollowWorker(context: Context, params: WorkerParameters) : Worker(context, params) {
  override fun doWork(): Result {
    val dataDir = inputData.getString(DATA_DIR) ?: return Result.failure()
    val json = try {
      PranchaNative.checkFollowing(dataDir, applicationContext)
    } catch (e: Throwable) {
      Log.w("Prancha", "verificação das obras acompanhadas: $e")
      return Result.retry()
    }
    show(JSONArray(json))
    return Result.success()
  }

  @SuppressLint("MissingPermission")
  private fun show(notices: JSONArray) {
    if (notices.length() == 0) return
    val context = applicationContext
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
      ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
    ) {
      // Sem permissão, os avisos ficam só na central do app.
      return
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      val channel = NotificationChannel(CHANNEL, "Capítulos novos", NotificationManager.IMPORTANCE_DEFAULT)
      channel.description = "Obras acompanhadas com capítulos novos, verificadas com o app fechado."
      context.getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }
    val open = context.packageManager.getLaunchIntentForPackage(context.packageName)?.let {
      PendingIntent.getActivity(context, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }
    val manager = NotificationManagerCompat.from(context)
    for (i in 0 until notices.length()) {
      val notice = notices.getJSONObject(i)
      val body = notice.optString("body")
      val notification = NotificationCompat.Builder(context, CHANNEL)
        .setSmallIcon(android.R.drawable.stat_notify_more)
        .setContentTitle(notice.optString("title"))
        .setContentText(body)
        .setStyle(NotificationCompat.BigTextStyle().bigText(body))
        .setAutoCancel(true)
        .setContentIntent(open)
        .build()
      manager.notify(notice.optLong("id").toInt(), notification)
    }
  }

  companion object {
    private const val DATA_DIR = "dataDir"
    private const val CHANNEL = "capitulos-novos"
    private const val WORK = "obras-acompanhadas"

    /** Agenda (ou mantém) a verificação. Chamado a cada abertura do app, com a pasta de dados dele. */
    fun schedule(context: Context, dataDir: String) {
      val constraints = Constraints.Builder()
        .setRequiredNetworkType(NetworkType.CONNECTED)
        .setRequiresBatteryNotLow(true)
        .build()
      val request = PeriodicWorkRequestBuilder<FollowWorker>(6, TimeUnit.HOURS)
        .setConstraints(constraints)
        .setInputData(workDataOf(DATA_DIR to dataDir))
        .build()
      WorkManager.getInstance(context).enqueueUniquePeriodicWork(WORK, ExistingPeriodicWorkPolicy.UPDATE, request)
    }
  }
}
