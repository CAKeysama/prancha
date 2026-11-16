package app.prancha

import android.app.Activity
import android.graphics.Color
import android.os.Build
import android.view.View
import android.view.WindowManager
import android.webkit.WebView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class ImmersiveArgs {
  var on: Boolean = false
}

@InvokeArg
class BarsArgs {
  var dark: Boolean = true
  lateinit var color: String
}

/**
 * A tela do app no Android: o conteúdo fica entre as barras do sistema (a interface não desenha embaixo delas),
 * o leitor usa a tela inteira e o Voltar na tela inicial manda o app para segundo plano, como os apps do sistema.
 * O Rust chama estes comandos (crates/prancha-app/src/android.rs).
 */
@TauriPlugin
class ScreenPlugin(private val activity: Activity) : Plugin(activity) {
  private val content: View get() = activity.findViewById(android.R.id.content)

  override fun load(webView: WebView) {
    activity.runOnUiThread {
      // Com a tela de ponta a ponta (MainActivity), o espaço das barras e do teclado vira margem do conteúdo. No
      // modo imersivo as barras somem, a margem vai a zero e a página ocupa a tela toda.
      ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
        val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
        val keyboard = insets.getInsets(WindowInsetsCompat.Type.ime())
        view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, keyboard.bottom))
        WindowInsetsCompat.CONSUMED
      }
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        activity.window.attributes = activity.window.attributes.apply {
          layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
        }
      }
      ViewCompat.requestApplyInsets(content)
    }
  }

  private fun controller() = WindowCompat.getInsetsController(activity.window, activity.window.decorView)

  /** Leitor: some com as barras do sistema; deslizar da borda mostra elas por um instante. */
  @Command
  fun setImmersive(invoke: Invoke) {
    val on = invoke.parseArgs(ImmersiveArgs::class.java).on
    activity.runOnUiThread {
      val c = controller()
      if (on) {
        c.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        c.hide(WindowInsetsCompat.Type.systemBars())
      } else {
        c.show(WindowInsetsCompat.Type.systemBars())
      }
      invoke.resolve()
    }
  }

  /** A faixa atrás das barras na cor do tema do app, com ícones claros ou escuros conforme o tema. */
  @Command
  fun setBars(invoke: Invoke) {
    val args = invoke.parseArgs(BarsArgs::class.java)
    activity.runOnUiThread {
      try {
        content.setBackgroundColor(Color.parseColor(args.color))
      } catch (e: IllegalArgumentException) {
        // Cor que o Android não entende: fica a do tema do sistema.
      }
      val c = controller()
      c.isAppearanceLightStatusBars = !args.dark
      c.isAppearanceLightNavigationBars = !args.dark
      invoke.resolve()
    }
  }

  /** Voltar na tela inicial: o app vai para segundo plano em vez de fechar, e volta onde estava. */
  @Command
  fun moveToBack(invoke: Invoke) {
    activity.runOnUiThread {
      activity.moveTaskToBack(true)
      invoke.resolve()
    }
  }
}
