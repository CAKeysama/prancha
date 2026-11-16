package app.prancha

import android.annotation.SuppressLint
import android.app.Activity
import android.graphics.Color
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import eu.kanade.tachiyomi.network.NetworkHelper
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicInteger

/**
 * A tela do site para a pessoa passar pela verificação anti-robô, como a janela do Windows. Quem responde é a pessoa,
 * à mão: o Prancha não resolve por ninguém. O navegador é o do sistema, com o mesmo identificador e os mesmos cookies
 * que as extensões do Mihon usam, então a liberação vale para elas. A tela fecha sozinha quando o site libera.
 */
class VerifyActivity : Activity() {
  private lateinit var web: WebView
  private val poll = Handler(Looper.getMainLooper())
  private var url = ""

  @SuppressLint("SetJavaScriptEnabled")
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    url = intent.getStringExtra(URL).orEmpty()
    val bar = LinearLayout(this).apply {
      orientation = LinearLayout.HORIZONTAL
      gravity = Gravity.CENTER_VERTICAL
      setPadding(32, 24, 16, 24)
      setBackgroundColor(Color.parseColor("#161B22"))
    }
    bar.addView(TextView(this).apply {
      text = "Confirme no site e toque em Pronto"
      setTextColor(Color.WHITE)
      textSize = 15f
    }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
    bar.addView(Button(this).apply {
      text = "Pronto"
      setOnClickListener { finish() }
    })
    web = WebView(this).apply {
      settings.javaScriptEnabled = true
      settings.domStorageEnabled = true
      settings.userAgentString = NetworkHelper.USER_AGENT
      webViewClient = WebViewClient()
    }
    CookieManager.getInstance().setAcceptThirdPartyCookies(web, true)
    val root = LinearLayout(this).apply {
      orientation = LinearLayout.VERTICAL
      fitsSystemWindows = true
      addView(bar, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
      addView(web, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
    }
    setContentView(root)
    if (url.startsWith("https://") || url.startsWith("http://")) web.loadUrl(url) else finish()
    poll.postDelayed(object : Runnable {
      override fun run() {
        if (isFinishing) return
        if (MihonExtensions.hasClearance(url)) {
          finish()
        } else {
          poll.postDelayed(this, 500)
        }
      }
    }, 500)
  }

  override fun onDestroy() {
    poll.removeCallbacksAndMessages(null)
    CookieManager.getInstance().flush()
    web.destroy()
    super.onDestroy()
    waiting.remove(intent.getIntExtra(TICKET, -1))?.countDown()
  }

  companion object {
    const val URL = "url"
    const val TICKET = "ticket"
    private val next = AtomicInteger()
    private val waiting = ConcurrentHashMap<Int, CountDownLatch>()

    /** Quem abriu a tela espera neste trinco, que solta quando ela fecha. */
    fun waitFor(done: CountDownLatch): Int = next.incrementAndGet().also { waiting[it] = done }
  }
}
