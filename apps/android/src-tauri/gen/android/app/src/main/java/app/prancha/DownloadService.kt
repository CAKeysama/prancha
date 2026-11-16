package app.prancha

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat

/**
 * Mantém o app vivo enquanto a fila de downloads tem capítulos: sem ele, o Android congela e encerra o processo
 * pouco depois de o app sair da tela, e o download para no meio. O Rust liga e desliga pelo BackgroundPlugin.
 */
class DownloadService : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val manager = getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      val channel = NotificationChannel(CHANNEL, "Downloads", NotificationManager.IMPORTANCE_LOW)
      channel.description = "Aparece enquanto o Prancha baixa capítulos."
      manager.createNotificationChannel(channel)
    }
    val open = packageManager.getLaunchIntentForPackage(packageName)?.let {
      PendingIntent.getActivity(this, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }
    val notification = NotificationCompat.Builder(this, CHANNEL)
      .setSmallIcon(android.R.drawable.stat_sys_download)
      .setContentTitle("Baixando capítulos")
      .setContentText("Continua com o Prancha fechado. A lista fica em Mais › Downloads.")
      .setProgress(0, 0, true)
      .setOngoing(true)
      .setOnlyAlertOnce(true)
      .setContentIntent(open)
      .build()
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
      startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
    } else {
      startForeground(NOTIFICATION_ID, notification)
    }
    return START_NOT_STICKY
  }

  /** Android 15 ou mais novo: o sistema limita esse tipo de serviço a 6 horas por dia. A fila continua quando o app abrir. */
  override fun onTimeout(startId: Int, fgsType: Int) {
    stopSelf()
  }

  companion object {
    private const val CHANNEL = "downloads"
    private const val NOTIFICATION_ID = 7301
  }
}
