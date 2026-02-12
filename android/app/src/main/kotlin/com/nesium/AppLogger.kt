package com.nesium

import android.content.Context
import android.content.Intent
import android.util.Log
import androidx.core.content.FileProvider
import java.io.File
import java.io.FileWriter
import java.io.PrintWriter
import java.text.SimpleDateFormat
import java.util.*
import java.util.concurrent.ConcurrentLinkedQueue

/**
 * App-wide logger that writes to both logcat and a file for easy sharing.
 */
object AppLogger {
    private const val TAG = "Nesium"
    private const val MAX_LOG_LINES = 5000
    private const val LOG_FILENAME = "nesium_debug.log"

    private var logFile: File? = null
    private var printWriter: PrintWriter? = null
    private val logBuffer = ConcurrentLinkedQueue<String>()
    private val dateFormat = SimpleDateFormat("HH:mm:ss.SSS", Locale.US)
    private val fileDateFormat = SimpleDateFormat("yyyy-MM-dd HH:mm:ss.SSS", Locale.US)

    /**
     * Initialize the logger with a context to get the cache directory.
     */
    fun init(context: Context) {
        try {
            logFile = File(context.cacheDir, LOG_FILENAME)

            // Create new log file (overwrite old one)
            printWriter = PrintWriter(FileWriter(logFile, false))

            val header = buildString {
                appendLine("=".repeat(60))
                appendLine("Nesium Debug Log")
                appendLine("Started: ${fileDateFormat.format(Date())}")
                appendLine("Device: ${android.os.Build.MANUFACTURER} ${android.os.Build.MODEL}")
                appendLine("Android: ${android.os.Build.VERSION.RELEASE} (SDK ${android.os.Build.VERSION.SDK_INT})")
                appendLine("=".repeat(60))
                appendLine()
            }
            printWriter?.print(header)
            printWriter?.flush()

            i("Logger initialized - log file: ${logFile?.absolutePath}")
        } catch (e: Exception) {
            Log.e(TAG, "Failed to initialize log file", e)
        }
    }

    fun i(message: String) { log("I", message) }
    fun d(message: String) { log("D", message) }
    fun w(message: String) { log("W", message) }

    fun e(message: String, throwable: Throwable? = null) {
        log("E", message)
        throwable?.let { log("E", it.stackTraceToString()) }
    }

    private fun log(level: String, message: String) {
        val timestamp = dateFormat.format(Date())
        val formattedMessage = "[$timestamp] $level: $message"

        when (level) {
            "I" -> Log.i(TAG, message)
            "D" -> Log.d(TAG, message)
            "W" -> Log.w(TAG, message)
            "E" -> Log.e(TAG, message)
        }

        logBuffer.add(formattedMessage)
        while (logBuffer.size > MAX_LOG_LINES) { logBuffer.poll() }

        try {
            printWriter?.println(formattedMessage)
            printWriter?.flush()
        } catch (e: Exception) {
            Log.e(TAG, "Failed to write to log file", e)
        }
    }

    fun getRecentLogs(maxLines: Int = 100): String {
        return logBuffer.toList().takeLast(maxLines).joinToString("\n")
    }

    fun shareLogFile(context: Context) {
        val file = logFile ?: return
        try {
            printWriter?.flush()
            val uri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", file)
            val shareIntent = Intent(Intent.ACTION_SEND).apply {
                type = "text/plain"
                putExtra(Intent.EXTRA_STREAM, uri)
                putExtra(Intent.EXTRA_SUBJECT, "Nesium Debug Log")
                putExtra(Intent.EXTRA_TEXT, "Nesium debug log attached.\n\nRecent entries:\n${getRecentLogs(20)}")
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            context.startActivity(Intent.createChooser(shareIntent, "Share Nesium Log"))
        } catch (e: Exception) {
            Log.e(TAG, "Failed to share log file", e)
        }
    }

    fun close() {
        try { printWriter?.close() } catch (e: Exception) { Log.e(TAG, "Failed to close log file", e) }
    }
}
