package tokyo.runo.aruarudb

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Bundle
import android.os.PowerManager
import android.widget.Button
import android.widget.EditText
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/**
 * aruaru-db Android版モニタリング/管理クライアント(参照実装
 * `open-web-server/android/`のMainActivity.ktと同じ電源プロファイル・
 * BroadcastReceiverパターンを踏襲)。
 *
 * **重要な位置づけ**: このActivityはaruaru-db本体(Rust製の分散DB
 * サーバー、Multi-Raft/HTAP OlapCache等)をAndroid上で実行するものでは
 * ない。ユーザーが入力したリモートURL(既存のaruaru-db管理API、
 * `crates/aruaru-server/src/admin.rs`の`GET /admin/cluster`等)へHTTPで
 * 接続し、疎通確認とクラスタ状態(ノード数・生存ノード数・レンジ数等)を
 * 画面上に表示する、リモート管理クライアントとして設計する。
 *
 * スコープ(意図的に含めない、詳細はリポジトリ`CLAUDE.md`のHANDOFF節参照):
 * 認証済み管理操作(バックアップ・移行・クラスタ再編成等の実行)、
 * 複数クラスタの同時監視、プッシュ通知。
 */
class MainActivity : AppCompatActivity() {

    companion object {
        const val EXTRA_PROFILE = "profile"
    }

    /**
     * プロファイル別の監視ポーリング間隔(open-web-server/android版の
     * `healthPollIntervalMs`と同じ考え方)。省電力版は間隔を大きく延ばし
     * (Doze/App Standbyへの影響を最小化)、常時電源接続版は短い間隔で
     * 即応性を優先する。
     */
    private fun pollIntervalMs(profile: PowerProfile): Long = when (profile) {
        PowerProfile.POWER_SAVE -> 5 * 60_000L // 5分
        PowerProfile.NORMAL -> 60_000L // 1分
        PowerProfile.ALWAYS_ON -> 5_000L // 5秒
    }

    private var wakeLock: PowerManager.WakeLock? = null
    private var pollJob: Job? = null
    private var powerConnectionReceiver: BroadcastReceiver? = null
    private lateinit var currentProfile: PowerProfile

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        currentProfile = resolveProfile()
        PowerProfile.save(this, currentProfile)

        val statusText = findViewById<TextView>(R.id.statusText)
        val logText = findViewById<TextView>(R.id.logText)
        val serverUrlInput = findViewById<EditText>(R.id.serverUrlInput)
        val connectButton = findViewById<Button>(R.id.connectButton)
        val changeProfileButton = findViewById<Button>(R.id.changeProfileButton)

        serverUrlInput.setText(PowerProfile.loadServerUrl(this))
        statusText.text = "aruaru-db monitor [${currentProfile.emoji} ${currentProfile.label}モード] (未接続)"

        connectButton.setOnClickListener {
            val url = serverUrlInput.text.toString().trim().trimEnd('/')
            if (url.isEmpty()) {
                Toast.makeText(this, "接続先URLを入力してください", Toast.LENGTH_SHORT).show()
                return@setOnClickListener
            }
            PowerProfile.saveServerUrl(this, url)
            connectButton.isEnabled = false
            CoroutineScope(Dispatchers.Main).launch {
                statusText.text = "[${currentProfile.emoji} ${currentProfile.label}] 接続確認中..."
                val log = StringBuilder()
                val ok = withContext(Dispatchers.IO) { checkClusterStatus(url, log) }
                statusText.text = if (ok) {
                    "[${currentProfile.emoji} ${currentProfile.label}] 接続OK"
                } else {
                    "[${currentProfile.emoji} ${currentProfile.label}] 接続失敗(ログ参照)"
                }
                logText.text = log.toString()
                connectButton.isEnabled = true
                if (ok) {
                    applyProfilePowerBehavior(log)
                    logText.text = log.toString()
                    startPeriodicPolling(url, statusText, logText)
                }
            }
        }

        changeProfileButton.setOnClickListener {
            startActivity(Intent(this, ProfileSelectActivity::class.java))
            finish()
        }

        registerPowerConnectionReceiver()
    }

    private fun resolveProfile(): PowerProfile {
        return when (intent?.action) {
            "tokyo.runo.aruarudb.LAUNCH_POWER_SAVE" -> PowerProfile.POWER_SAVE
            "tokyo.runo.aruarudb.LAUNCH_NORMAL" -> PowerProfile.NORMAL
            "tokyo.runo.aruarudb.LAUNCH_ALWAYS_ON" -> PowerProfile.ALWAYS_ON
            else -> {
                val extra = intent?.getStringExtra(EXTRA_PROFILE)
                if (extra != null) PowerProfile.fromPrefValue(extra) else PowerProfile.load(this)
            }
        }
    }

    /**
     * プロファイルごとの電源管理の中身(open-web-server/android版の
     * `applyProfilePowerBehavior`と同じ設計)。省電力/通常はWakeLockを
     * 取得しない、常時電源接続のみ`PARTIAL_WAKE_LOCK`を保持する。
     */
    private fun applyProfilePowerBehavior(log: StringBuilder) {
        when (currentProfile) {
            PowerProfile.ALWAYS_ON -> {
                try {
                    val pm = getSystemService(POWER_SERVICE) as PowerManager
                    val lock = pm.newWakeLock(
                        PowerManager.PARTIAL_WAKE_LOCK,
                        "AruaruDbMonitor::AlwaysOnWakeLock"
                    )
                    lock.acquire()
                    wakeLock = lock
                    log.appendLine("power: acquired PARTIAL_WAKE_LOCK (always-on profile)")
                } catch (e: Exception) {
                    log.appendLine("power: failed to acquire WakeLock: ${e.message}")
                }
            }
            PowerProfile.POWER_SAVE -> {
                log.appendLine("power: no WakeLock acquired (power-save profile, Doze-friendly)")
            }
            PowerProfile.NORMAL -> {
                log.appendLine("power: no WakeLock acquired (normal profile)")
            }
        }
    }

    /**
     * 電源の抜き差し監視(open-web-server/android版と同じダイアログ導線)。
     * 常時電源接続版実行中に電源が外れたら省電力/通常への切替を尋ね、
     * 逆に電源が再接続されたら常時電源接続への切替を尋ねる。
     */
    private fun registerPowerConnectionReceiver() {
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                when (intent.action) {
                    Intent.ACTION_POWER_DISCONNECTED -> onPowerDisconnected()
                    Intent.ACTION_POWER_CONNECTED -> onPowerConnected()
                }
            }
        }
        powerConnectionReceiver = receiver
        val filter = IntentFilter().apply {
            addAction(Intent.ACTION_POWER_DISCONNECTED)
            addAction(Intent.ACTION_POWER_CONNECTED)
        }
        registerReceiver(receiver, filter)
    }

    private fun onPowerDisconnected() {
        if (currentProfile != PowerProfile.ALWAYS_ON) return
        if (isFinishing || isDestroyed) return
        AlertDialog.Builder(this)
            .setTitle("電源が外れました")
            .setMessage(
                "常時電源接続モードで監視中に電源が外れました。\n" +
                    "省電力モードに切り替えますか?それとも通常モードの" +
                    "ままにしますか?\n(推奨: 省電力モード)"
            )
            .setPositiveButton("省電力モードへ切替") { _, _ ->
                switchProfileAndRestart(PowerProfile.POWER_SAVE)
            }
            .setNegativeButton("通常モードのままにする") { _, _ ->
                switchProfileAndRestart(PowerProfile.NORMAL)
            }
            .setCancelable(false)
            .show()
    }

    private fun onPowerConnected() {
        if (currentProfile == PowerProfile.ALWAYS_ON) return
        if (isFinishing || isDestroyed) return
        AlertDialog.Builder(this)
            .setTitle("電源が接続されました")
            .setMessage("常時電源接続モード(短間隔監視)に切り替えますか?")
            .setPositiveButton("常時電源接続へ切替") { _, _ ->
                switchProfileAndRestart(PowerProfile.ALWAYS_ON)
            }
            .setNegativeButton("このままにする", null)
            .show()
    }

    private fun switchProfileAndRestart(newProfile: PowerProfile) {
        PowerProfile.save(this, newProfile)
        Toast.makeText(
            this,
            "${newProfile.emoji} ${newProfile.label}モードへ切り替えます",
            Toast.LENGTH_SHORT
        ).show()
        val intent = Intent(this, MainActivity::class.java)
        intent.putExtra(EXTRA_PROFILE, newProfile.prefValue)
        startActivity(intent)
        finish()
    }

    /**
     * aruaru-db管理API `GET /admin/cluster`(`crates/aruaru-server/src/
     * admin.rs::cluster_status`)へ接続し、クラスタの統計情報を取得する。
     * このAPIはaruaru-db本体が既に提供しているエンドポイントであり、この
     * アプリはそれをHTTP経由で叩くだけ(サーバー機能自体は実装しない)。
     */
    private fun checkClusterStatus(baseUrl: String, log: StringBuilder): Boolean {
        return try {
            val url = URL("$baseUrl/admin/cluster")
            val conn = url.openConnection() as HttpURLConnection
            conn.connectTimeout = 5000
            conn.readTimeout = 5000
            conn.requestMethod = "GET"
            val code = conn.responseCode
            log.appendLine("GET $url -> $code")
            if (code == 200) {
                val body = conn.inputStream.bufferedReader().readText()
                val json = JSONObject(body)
                val stats = json.optJSONObject("stats")
                if (stats != null) {
                    log.appendLine("total_nodes: ${stats.optInt("total_nodes")}")
                    log.appendLine("healthy_nodes: ${stats.optInt("healthy_nodes")}")
                    log.appendLine("total_ranges: ${stats.optInt("total_ranges")}")
                    log.appendLine("table_count: ${stats.optInt("table_count")}")
                    log.appendLine("under_replicated: ${stats.optBoolean("under_replicated")}")
                }
                conn.disconnect()
                true
            } else {
                conn.disconnect()
                false
            }
        } catch (e: Exception) {
            log.appendLine("ERROR: ${e.message}")
            false
        }
    }

    /**
     * 継続的なクラスタ監視ループ(open-web-server/android版の
     * `startPeriodicHealthPoll`と同じ設計)。プロファイルごとに間隔を
     * 変えることが「省電力版が実際に省電力になる」施策そのもの。
     */
    private fun startPeriodicPolling(baseUrl: String, statusText: TextView, logText: TextView) {
        pollJob?.cancel()
        val intervalMs = pollIntervalMs(currentProfile)
        pollJob = CoroutineScope(Dispatchers.Main).launch {
            while (isActive) {
                delay(intervalMs)
                val log = StringBuilder()
                val ok = withContext(Dispatchers.IO) { checkClusterStatus(baseUrl, log) }
                statusText.text = if (ok) {
                    "[${currentProfile.emoji} ${currentProfile.label}] 監視中 " +
                        "(${intervalMs / 1000}秒間隔)"
                } else {
                    "[${currentProfile.emoji} ${currentProfile.label}] 接続失敗"
                }
                logText.text = log.toString()
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        pollJob?.cancel()
        powerConnectionReceiver?.let {
            try {
                unregisterReceiver(it)
            } catch (_: IllegalArgumentException) {
                // 未登録のまま呼ばれても(onCreateの早期return等)無視する。
            }
        }
        if (wakeLock?.isHeld == true) {
            wakeLock?.release()
        }
    }
}
