package com.nesium

import android.content.Intent
import android.graphics.Bitmap
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.net.Uri
import android.os.Bundle
import android.view.WindowManager
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.*

// Scaling modes for the game screen
enum class ScaleMode(val displayName: String) {
    FIT("Fit Screen"),
    PIXEL_PERFECT("Pixel Perfect"),
    INTEGER("Integer Scale"),
    STRETCH("Stretch"),
    SCANLINE("Scanline")
}

// NES-inspired color palette
object NESColors {
    // Shell colors (NES controller gray)
    val shellPrimary = Color(0xFF2A2A3A)
    val shellDark = Color(0xFF1A1A2A)
    val shellLight = Color(0xFF3A3A4A)
    val shellAccent = Color(0xFF4A4A5A)

    // Screen bezel
    val screenBezel = Color(0xFF1A1A1A)
    val screenFrame = Color(0xFF0A0A0A)

    // Button colors
    val dpadColor = Color(0xFF1A1A1A)
    val dpadHighlight = Color(0xFF2A2A2A)
    val buttonA = Color(0xFFCC0000)       // NES red
    val buttonB = Color(0xFFCC0000)       // NES red
    val buttonStart = Color(0xFF3A3A4A)
    val buttonPressed = Color(0xFF666666)

    // Text colors
    val textPrimary = Color(0xFFE0E0E0)
    val textSecondary = Color(0xFFB0B0B0)
    val textAccent = Color(0xFFFF4444)     // Red accent

    // Status
    val ledOn = Color(0xFFFF0000)
}

class MainActivity : ComponentActivity() {

    private var emulationJob: Job? = null
    private var audioTrack: AudioTrack? = null

    // NES resolution: 256x240
    private val framebuffer = IntArray(256 * 240)
    private val audioSamples = FloatArray(4096)

    private var _romLoaded = mutableStateOf(false)

    private val romPickerLauncher = registerForActivityResult(
        ActivityResultContracts.OpenDocument()
    ) { uri ->
        uri?.let {
            AppLogger.d("ROM picker returned URI: $it")
            loadRomFromUri(it)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        AppLogger.init(this)
        AppLogger.i("Nesium Android v1.0 starting...")

        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

        try {
            NesiumCore.initLogging()
            AppLogger.i("Native core initialized")
        } catch (e: Exception) {
            AppLogger.e("Failed to init native core", e)
        }

        initAudio()

        setContent {
            NesiumApp()
        }

        handleIntent(intent)
    }

    @Composable
    fun NesiumApp() {
        val context = LocalContext.current
        val romLoaded by _romLoaded
        var bitmap by remember { mutableStateOf<Bitmap?>(null) }
        var isPaused by remember { mutableStateOf(false) }
        var fps by remember { mutableStateOf(0) }
        var statusText by remember { mutableStateOf("") }
        var showLogDialog by remember { mutableStateOf(false) }
        var showSettingsDialog by remember { mutableStateOf(false) }
        var scaleMode by remember { mutableStateOf(ScaleMode.FIT) }

        // Initialize bitmap
        LaunchedEffect(Unit) {
            bitmap = Bitmap.createBitmap(256, 240, Bitmap.Config.ARGB_8888)
            val blackScreen = IntArray(256 * 240) { 0xFF000000.toInt() }
            bitmap?.setPixels(blackScreen, 0, 256, 0, 0, 256, 240)
        }

        // Log dialog
        if (showLogDialog) {
            LogViewerDialog(
                onDismiss = { showLogDialog = false },
                onShare = { AppLogger.shareLogFile(context) }
            )
        }

        // Settings dialog
        if (showSettingsDialog) {
            SettingsDialog(
                currentScaleMode = scaleMode,
                onScaleModeChanged = { scaleMode = it },
                onDismiss = { showSettingsDialog = false }
            )
        }

        // Emulation loop
        LaunchedEffect(romLoaded, isPaused) {
            if (romLoaded && !isPaused) {
                AppLogger.i("Starting emulation")
                emulationJob?.cancel()
                emulationJob = lifecycleScope.launch(Dispatchers.Default) {
                    val targetFrameTime = 1000L / 60
                    var fpsTimer = System.currentTimeMillis()
                    var fpsFrames = 0

                    while (isActive && NesiumCore.isRomLoaded()) {
                        val startTime = System.currentTimeMillis()

                        try {
                            val result = NesiumCore.runFrame(framebuffer)
                            fpsFrames++

                            val now = System.currentTimeMillis()
                            if (now - fpsTimer >= 1000) {
                                val currentFps = fpsFrames
                                fpsFrames = 0
                                fpsTimer = now
                                withContext(Dispatchers.Main) {
                                    fps = currentFps
                                    statusText = "$currentFps FPS"
                                }
                            }

                            withContext(Dispatchers.Main) {
                                bitmap = Bitmap.createBitmap(256, 240, Bitmap.Config.ARGB_8888).also {
                                    it.setPixels(framebuffer, 0, 256, 0, 0, 256, 240)
                                }
                            }

                            val sampleCount = NesiumCore.getAudioSamples(audioSamples)
                            if (sampleCount > 0) {
                                playAudio(sampleCount)
                            }
                        } catch (e: Exception) {
                            AppLogger.e("Emulation error", e)
                        }

                        val elapsed = System.currentTimeMillis() - startTime
                        if (elapsed < targetFrameTime) {
                            delay(targetFrameTime - elapsed)
                        }
                    }
                }
            } else {
                emulationJob?.cancel()
                statusText = if (romLoaded) "PAUSED" else ""
            }
        }

        // Main UI
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    Brush.verticalGradient(
                        colors = listOf(NESColors.shellPrimary, NESColors.shellDark)
                    )
                )
        ) {
            Column(
                modifier = Modifier.fillMaxSize(),
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                // Top section with screen
                ScreenSection(
                    bitmap = bitmap,
                    romLoaded = romLoaded,
                    statusText = statusText,
                    scaleMode = scaleMode,
                    onLoadRom = { romPickerLauncher.launch(arrayOf("*/*")) },
                    modifier = Modifier.weight(0.55f)
                )

                // Controls section
                ControlsSection(
                    romLoaded = romLoaded,
                    isPaused = isPaused,
                    onPauseToggle = { isPaused = !isPaused },
                    onShowLogs = { showLogDialog = true },
                    onShowSettings = { showSettingsDialog = true },
                    onLoadRom = { romPickerLauncher.launch(arrayOf("*/*")) },
                    modifier = Modifier.weight(0.45f)
                )
            }
        }
    }

    @Composable
    fun ScreenSection(
        bitmap: Bitmap?,
        romLoaded: Boolean,
        statusText: String,
        scaleMode: ScaleMode,
        onLoadRom: () -> Unit,
        modifier: Modifier = Modifier
    ) {
        Column(
            modifier = modifier
                .fillMaxWidth()
                .padding(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            // Brand text
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.padding(bottom = 8.dp)
            ) {
                Text(
                    text = "NESIUM",
                    color = NESColors.textAccent,
                    fontSize = 14.sp,
                    fontWeight = FontWeight.Bold,
                    letterSpacing = 6.sp
                )
                Spacer(modifier = Modifier.width(8.dp))
                Text(
                    text = "NES EMULATOR",
                    color = NESColors.textSecondary.copy(alpha = 0.7f),
                    fontSize = 9.sp,
                    fontWeight = FontWeight.Medium,
                    letterSpacing = 2.sp
                )
            }

            // Screen bezel
            Box(
                modifier = Modifier
                    .fillMaxWidth(0.95f)
                    .weight(1f)
                    .shadow(8.dp, RoundedCornerShape(12.dp))
                    .clip(RoundedCornerShape(12.dp))
                    .background(NESColors.screenBezel)
                    .padding(12.dp),
                contentAlignment = Alignment.Center
            ) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    modifier = Modifier.fillMaxSize()
                ) {
                    // Power LED and status row
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(bottom = 6.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        // Power LED
                        Box(
                            modifier = Modifier
                                .size(8.dp)
                                .clip(CircleShape)
                                .background(
                                    if (romLoaded) NESColors.ledOn else Color.DarkGray
                                )
                        )

                        Spacer(modifier = Modifier.width(6.dp))

                        Text(
                            text = "POWER",
                            color = NESColors.textSecondary,
                            fontSize = 8.sp,
                            fontWeight = FontWeight.Medium
                        )

                        Spacer(modifier = Modifier.weight(1f))

                        Text(
                            text = scaleMode.displayName,
                            color = NESColors.textSecondary,
                            fontSize = 8.sp
                        )

                        Spacer(modifier = Modifier.width(8.dp))

                        Text(
                            text = statusText,
                            color = NESColors.textAccent,
                            fontSize = 10.sp,
                            fontWeight = FontWeight.Bold
                        )
                    }

                    // Game screen
                    GameScreen(
                        bitmap = bitmap,
                        romLoaded = romLoaded,
                        scaleMode = scaleMode,
                        onLoadRom = onLoadRom,
                        modifier = Modifier
                            .fillMaxSize()
                            .clip(RoundedCornerShape(4.dp))
                            .background(NESColors.screenFrame)
                            .border(2.dp, Color.Black, RoundedCornerShape(4.dp))
                            .padding(2.dp)
                    )
                }
            }
        }
    }

    @Composable
    fun GameScreen(
        bitmap: Bitmap?,
        romLoaded: Boolean,
        scaleMode: ScaleMode,
        onLoadRom: () -> Unit,
        modifier: Modifier = Modifier
    ) {
        BoxWithConstraints(
            modifier = modifier
                .background(Color.Black)
                .clickable(enabled = !romLoaded) { onLoadRom() },
            contentAlignment = Alignment.Center
        ) {
            val containerWidth = maxWidth
            val containerHeight = maxHeight

            // NES aspect ratio: 256x240 = 16:15
            val gameWidth = 256f
            val gameHeight = 240f

            val (scaledWidth, scaledHeight) = when (scaleMode) {
                ScaleMode.PIXEL_PERFECT -> Pair(256.dp, 240.dp)
                ScaleMode.INTEGER -> {
                    val maxScaleX = (containerWidth.value / gameWidth).toInt()
                    val maxScaleY = (containerHeight.value / gameHeight).toInt()
                    val scale = maxOf(1, minOf(maxScaleX, maxScaleY))
                    Pair((gameWidth * scale).dp, (gameHeight * scale).dp)
                }
                ScaleMode.FIT -> {
                    val scaleX = containerWidth.value / gameWidth
                    val scaleY = containerHeight.value / gameHeight
                    val scale = minOf(scaleX, scaleY)
                    Pair((gameWidth * scale).dp, (gameHeight * scale).dp)
                }
                ScaleMode.STRETCH -> Pair(containerWidth, containerHeight)
                ScaleMode.SCANLINE -> {
                    val scaleX = containerWidth.value / gameWidth
                    val scaleY = containerHeight.value / gameHeight
                    val scale = minOf(scaleX, scaleY)
                    Pair((gameWidth * scale).dp, (gameHeight * scale).dp)
                }
            }

            if (bitmap != null && romLoaded) {
                Box(
                    modifier = Modifier.size(scaledWidth, scaledHeight)
                ) {
                    Image(
                        bitmap = bitmap.asImageBitmap(),
                        contentDescription = "Game Screen",
                        modifier = Modifier.fillMaxSize(),
                        contentScale = androidx.compose.ui.layout.ContentScale.FillBounds,
                        filterQuality = when (scaleMode) {
                            ScaleMode.PIXEL_PERFECT, ScaleMode.INTEGER ->
                                androidx.compose.ui.graphics.FilterQuality.None
                            else ->
                                androidx.compose.ui.graphics.FilterQuality.Low
                        }
                    )

                    // Scanline overlay
                    if (scaleMode == ScaleMode.SCANLINE) {
                        ScanlineOverlay(modifier = Modifier.fillMaxSize())
                    }
                }
            } else {
                // Empty screen / load prompt
                Box(
                    modifier = Modifier
                        .size(scaledWidth, scaledHeight)
                        .background(Color(0xFF111111)),
                    contentAlignment = Alignment.Center
                ) {
                    Column(
                        horizontalAlignment = Alignment.CenterHorizontally
                    ) {
                        Text(
                            text = "\uD83C\uDFAE",
                            fontSize = 32.sp
                        )
                        Spacer(modifier = Modifier.height(8.dp))
                        Text(
                            text = "TAP TO LOAD ROM",
                            color = NESColors.textAccent,
                            fontSize = 12.sp,
                            fontWeight = FontWeight.Bold,
                            letterSpacing = 1.sp
                        )
                        Spacer(modifier = Modifier.height(4.dp))
                        Text(
                            text = ".nes files supported",
                            color = NESColors.textSecondary,
                            fontSize = 10.sp
                        )
                    }
                }
            }
        }
    }

    @Composable
    fun ScanlineOverlay(modifier: Modifier = Modifier) {
        Canvas(modifier = modifier) {
            val lineSpacing = 2.dp.toPx()
            var y = 0f
            while (y < size.height) {
                drawLine(
                    color = Color.Black.copy(alpha = 0.2f),
                    start = Offset(0f, y),
                    end = Offset(size.width, y),
                    strokeWidth = 1.dp.toPx()
                )
                y += lineSpacing
            }
        }
    }

    @Composable
    fun ControlsSection(
        romLoaded: Boolean,
        isPaused: Boolean,
        onPauseToggle: () -> Unit,
        onShowLogs: () -> Unit,
        onShowSettings: () -> Unit,
        onLoadRom: () -> Unit,
        modifier: Modifier = Modifier
    ) {
        Box(
            modifier = modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 8.dp)
        ) {
            // D-Pad (left side)
            DPad(
                modifier = Modifier
                    .align(Alignment.CenterStart)
                    .offset(x = 20.dp)
            )

            // A/B Buttons (right side)
            ABButtons(
                modifier = Modifier
                    .align(Alignment.CenterEnd)
                    .offset(x = (-20).dp, y = (-10).dp)
            )

            // Start/Select (bottom center)
            StartSelectButtons(
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .offset(y = (-20).dp)
            )

            // Menu buttons (top)
            Row(
                modifier = Modifier
                    .align(Alignment.TopCenter)
                    .padding(top = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                MenuButton(text = "LOAD", onClick = onLoadRom)

                if (romLoaded) {
                    MenuButton(
                        text = if (isPaused) "PLAY" else "PAUSE",
                        onClick = onPauseToggle
                    )
                }

                MenuButton(text = "SETTINGS", onClick = onShowSettings)
                MenuButton(text = "LOG", onClick = onShowLogs)
            }
        }
    }

    @Composable
    fun DPad(modifier: Modifier = Modifier) {
        val size = 130.dp
        val buttonSize = 44.dp

        Box(
            modifier = modifier.size(size),
            contentAlignment = Alignment.Center
        ) {
            // D-Pad base
            Canvas(modifier = Modifier.size(size)) {
                val centerX = size.toPx() / 2
                val centerY = size.toPx() / 2
                val armWidth = buttonSize.toPx()
                val armLength = buttonSize.toPx() * 1.1f

                drawRoundRect(
                    color = NESColors.dpadColor,
                    topLeft = Offset(centerX - armWidth / 2, centerY - armLength),
                    size = androidx.compose.ui.geometry.Size(armWidth, armLength * 2),
                    cornerRadius = androidx.compose.ui.geometry.CornerRadius(8f, 8f)
                )

                drawRoundRect(
                    color = NESColors.dpadColor,
                    topLeft = Offset(centerX - armLength, centerY - armWidth / 2),
                    size = androidx.compose.ui.geometry.Size(armLength * 2, armWidth),
                    cornerRadius = androidx.compose.ui.geometry.CornerRadius(8f, 8f)
                )
            }

            // Touch areas
            DPadButton(
                modifier = Modifier.align(Alignment.TopCenter).size(buttonSize).offset(y = 4.dp),
                onPress = { NesiumCore.pressButton(NesiumCore.BUTTON_UP) },
                onRelease = { NesiumCore.releaseButton(NesiumCore.BUTTON_UP) }
            )
            DPadButton(
                modifier = Modifier.align(Alignment.BottomCenter).size(buttonSize).offset(y = (-4).dp),
                onPress = { NesiumCore.pressButton(NesiumCore.BUTTON_DOWN) },
                onRelease = { NesiumCore.releaseButton(NesiumCore.BUTTON_DOWN) }
            )
            DPadButton(
                modifier = Modifier.align(Alignment.CenterStart).size(buttonSize).offset(x = 4.dp),
                onPress = { NesiumCore.pressButton(NesiumCore.BUTTON_LEFT) },
                onRelease = { NesiumCore.releaseButton(NesiumCore.BUTTON_LEFT) }
            )
            DPadButton(
                modifier = Modifier.align(Alignment.CenterEnd).size(buttonSize).offset(x = (-4).dp),
                onPress = { NesiumCore.pressButton(NesiumCore.BUTTON_RIGHT) },
                onRelease = { NesiumCore.releaseButton(NesiumCore.BUTTON_RIGHT) }
            )
        }
    }

    @Composable
    fun DPadButton(
        modifier: Modifier = Modifier,
        onPress: () -> Unit,
        onRelease: () -> Unit
    ) {
        var pressed by remember { mutableStateOf(false) }

        Box(
            modifier = modifier
                .background(
                    if (pressed) NESColors.dpadHighlight.copy(alpha = 0.5f)
                    else Color.Transparent
                )
                .pointerInput(Unit) {
                    detectTapGestures(
                        onPress = {
                            pressed = true
                            onPress()
                            tryAwaitRelease()
                            pressed = false
                            onRelease()
                        }
                    )
                }
        )
    }

    @Composable
    fun ABButtons(modifier: Modifier = Modifier) {
        Box(
            modifier = modifier.size(140.dp, 100.dp)
        ) {
            // B button (lower left)
            ActionButton(
                label = "B",
                color = NESColors.buttonB,
                modifier = Modifier
                    .align(Alignment.CenterStart)
                    .offset(y = 15.dp),
                onPress = { NesiumCore.pressButton(NesiumCore.BUTTON_B) },
                onRelease = { NesiumCore.releaseButton(NesiumCore.BUTTON_B) }
            )

            // A button (upper right)
            ActionButton(
                label = "A",
                color = NESColors.buttonA,
                modifier = Modifier
                    .align(Alignment.CenterEnd)
                    .offset(y = (-15).dp),
                onPress = { NesiumCore.pressButton(NesiumCore.BUTTON_A) },
                onRelease = { NesiumCore.releaseButton(NesiumCore.BUTTON_A) }
            )
        }
    }

    @Composable
    fun ActionButton(
        label: String,
        color: Color,
        modifier: Modifier = Modifier,
        onPress: () -> Unit,
        onRelease: () -> Unit
    ) {
        var pressed by remember { mutableStateOf(false) }

        Box(
            modifier = modifier
                .size(58.dp)
                .shadow(if (pressed) 2.dp else 4.dp, CircleShape)
                .clip(CircleShape)
                .background(
                    Brush.radialGradient(
                        colors = if (pressed) {
                            listOf(color.copy(alpha = 0.7f), color.copy(alpha = 0.5f))
                        } else {
                            listOf(color, color.copy(alpha = 0.8f))
                        }
                    )
                )
                .border(2.dp, color.copy(alpha = 0.3f), CircleShape)
                .pointerInput(Unit) {
                    detectTapGestures(
                        onPress = {
                            pressed = true
                            onPress()
                            tryAwaitRelease()
                            pressed = false
                            onRelease()
                        }
                    )
                },
            contentAlignment = Alignment.Center
        ) {
            Text(
                text = label,
                color = Color.White,
                fontSize = 18.sp,
                fontWeight = FontWeight.Bold
            )
        }
    }

    @Composable
    fun StartSelectButtons(modifier: Modifier = Modifier) {
        Row(
            modifier = modifier,
            horizontalArrangement = Arrangement.spacedBy(24.dp)
        ) {
            PillButton(
                label = "SELECT",
                onPress = { NesiumCore.pressButton(NesiumCore.BUTTON_SELECT) },
                onRelease = { NesiumCore.releaseButton(NesiumCore.BUTTON_SELECT) }
            )
            PillButton(
                label = "START",
                onPress = { NesiumCore.pressButton(NesiumCore.BUTTON_START) },
                onRelease = { NesiumCore.releaseButton(NesiumCore.BUTTON_START) }
            )
        }
    }

    @Composable
    fun PillButton(
        label: String,
        onPress: () -> Unit,
        onRelease: () -> Unit
    ) {
        var pressed by remember { mutableStateOf(false) }

        Box(
            modifier = Modifier
                .width(56.dp)
                .height(18.dp)
                .shadow(if (pressed) 1.dp else 2.dp, RoundedCornerShape(9.dp))
                .clip(RoundedCornerShape(9.dp))
                .background(
                    if (pressed) NESColors.buttonPressed else NESColors.buttonStart
                )
                .pointerInput(Unit) {
                    detectTapGestures(
                        onPress = {
                            pressed = true
                            onPress()
                            tryAwaitRelease()
                            pressed = false
                            onRelease()
                        }
                    )
                },
            contentAlignment = Alignment.Center
        ) {
            Text(
                text = label,
                color = NESColors.textSecondary,
                fontSize = 8.sp,
                fontWeight = FontWeight.Bold,
                letterSpacing = 1.sp
            )
        }
    }

    @Composable
    fun MenuButton(text: String, onClick: () -> Unit) {
        TextButton(
            onClick = onClick,
            colors = ButtonDefaults.textButtonColors(
                contentColor = NESColors.textAccent
            ),
            contentPadding = PaddingValues(horizontal = 12.dp, vertical = 4.dp)
        ) {
            Text(
                text = text,
                fontSize = 10.sp,
                fontWeight = FontWeight.Bold,
                letterSpacing = 1.sp
            )
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    private fun handleIntent(intent: Intent?) {
        intent?.data?.let { uri ->
            AppLogger.d("Handling intent with URI: $uri")
            loadRomFromUri(uri)
        }
    }

    private fun loadRomFromUri(uri: Uri) {
        AppLogger.i("Loading ROM from URI: $uri")

        lifecycleScope.launch(Dispatchers.IO) {
            try {
                contentResolver.openInputStream(uri)?.use { inputStream ->
                    val bytes = inputStream.readBytes()
                    AppLogger.i("Read ${bytes.size} bytes")

                    if (bytes.size < 16) {
                        AppLogger.e("ROM too small: ${bytes.size} bytes")
                        withContext(Dispatchers.Main) {
                            Toast.makeText(this@MainActivity, "Invalid ROM file", Toast.LENGTH_SHORT).show()
                        }
                        return@use
                    }

                    val success = NesiumCore.loadRomFromBytes(bytes)
                    val isLoaded = NesiumCore.isRomLoaded()
                    AppLogger.i("Load result: success=$success, isLoaded=$isLoaded")

                    withContext(Dispatchers.Main) {
                        if (success && isLoaded) {
                            _romLoaded.value = true
                            Toast.makeText(this@MainActivity, "ROM loaded!", Toast.LENGTH_SHORT).show()
                        } else {
                            Toast.makeText(this@MainActivity, "Failed to load ROM", Toast.LENGTH_SHORT).show()
                        }
                    }
                }
            } catch (e: Exception) {
                AppLogger.e("Error loading ROM", e)
                withContext(Dispatchers.Main) {
                    Toast.makeText(this@MainActivity, "Error: ${e.message}", Toast.LENGTH_SHORT).show()
                }
            }
        }
    }

    private fun initAudio() {
        try {
            val sampleRate = 44100
            val bufferSize = AudioTrack.getMinBufferSize(
                sampleRate,
                AudioFormat.CHANNEL_OUT_MONO,
                AudioFormat.ENCODING_PCM_FLOAT
            )

            audioTrack = AudioTrack.Builder()
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_GAME)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                        .build()
                )
                .setAudioFormat(
                    AudioFormat.Builder()
                        .setSampleRate(sampleRate)
                        .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                        .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                        .build()
                )
                .setBufferSizeInBytes(bufferSize * 4)
                .setTransferMode(AudioTrack.MODE_STREAM)
                .build()

            audioTrack?.play()
            AppLogger.i("Audio initialized: $sampleRate Hz mono")
        } catch (e: Exception) {
            AppLogger.e("Failed to initialize audio", e)
        }
    }

    private fun playAudio(sampleCount: Int) {
        try {
            audioTrack?.write(audioSamples, 0, sampleCount, AudioTrack.WRITE_NON_BLOCKING)
        } catch (e: Exception) {
            // Silently ignore audio write errors
        }
    }

    override fun onPause() {
        super.onPause()
        emulationJob?.cancel()
        audioTrack?.pause()
    }

    override fun onResume() {
        super.onResume()
        audioTrack?.play()
    }

    override fun onDestroy() {
        super.onDestroy()
        emulationJob?.cancel()
        audioTrack?.release()
        NesiumCore.unloadRom()
        AppLogger.close()
    }
}

@Composable
fun SettingsDialog(
    currentScaleMode: ScaleMode,
    onScaleModeChanged: (ScaleMode) -> Unit,
    onDismiss: () -> Unit
) {
    Dialog(onDismissRequest = onDismiss) {
        Surface(
            modifier = Modifier
                .fillMaxWidth()
                .wrapContentHeight(),
            shape = RoundedCornerShape(16.dp),
            color = NESColors.shellDark
        ) {
            Column(modifier = Modifier.padding(20.dp)) {
                Text(
                    text = "Display Settings",
                    color = NESColors.textAccent,
                    fontSize = 20.sp,
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.padding(bottom = 16.dp)
                )

                Text(
                    text = "SCALING MODE",
                    color = NESColors.textSecondary,
                    fontSize = 11.sp,
                    fontWeight = FontWeight.Bold,
                    letterSpacing = 1.sp,
                    modifier = Modifier.padding(bottom = 12.dp)
                )

                ScaleMode.entries.forEach { mode ->
                    ScaleModeOption(
                        mode = mode,
                        isSelected = mode == currentScaleMode,
                        onSelect = { onScaleModeChanged(mode) }
                    )
                    if (mode != ScaleMode.entries.last()) {
                        Spacer(modifier = Modifier.height(4.dp))
                    }
                }

                Spacer(modifier = Modifier.height(20.dp))

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.End
                ) {
                    TextButton(
                        onClick = onDismiss,
                        colors = ButtonDefaults.textButtonColors(
                            contentColor = NESColors.textAccent
                        )
                    ) {
                        Text(
                            text = "DONE",
                            fontWeight = FontWeight.Bold,
                            letterSpacing = 1.sp
                        )
                    }
                }
            }
        }
    }
}

@Composable
fun ScaleModeOption(
    mode: ScaleMode,
    isSelected: Boolean,
    onSelect: () -> Unit
) {
    val description = when (mode) {
        ScaleMode.FIT -> "Scale to fit screen, keeps aspect ratio"
        ScaleMode.PIXEL_PERFECT -> "1:1 pixel mapping, may be small"
        ScaleMode.INTEGER -> "Largest whole number scale that fits"
        ScaleMode.STRETCH -> "Fill entire screen area"
        ScaleMode.SCANLINE -> "Fit with retro scanline overlay"
    }

    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(8.dp))
            .clickable { onSelect() },
        color = if (isSelected) NESColors.shellAccent else NESColors.shellPrimary,
        shape = RoundedCornerShape(8.dp)
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(12.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Box(
                modifier = Modifier
                    .size(20.dp)
                    .border(
                        width = 2.dp,
                        color = if (isSelected) NESColors.textAccent else NESColors.textSecondary,
                        shape = CircleShape
                    ),
                contentAlignment = Alignment.Center
            ) {
                if (isSelected) {
                    Box(
                        modifier = Modifier
                            .size(10.dp)
                            .background(NESColors.textAccent, CircleShape)
                    )
                }
            }

            Spacer(modifier = Modifier.width(12.dp))

            Column {
                Text(
                    text = mode.displayName,
                    color = if (isSelected) NESColors.textAccent else NESColors.textPrimary,
                    fontSize = 14.sp,
                    fontWeight = FontWeight.Medium
                )
                Text(
                    text = description,
                    color = NESColors.textSecondary,
                    fontSize = 11.sp
                )
            }
        }
    }
}

@Composable
fun LogViewerDialog(
    onDismiss: () -> Unit,
    onShare: () -> Unit
) {
    var logText by remember { mutableStateOf(AppLogger.getRecentLogs(200)) }

    LaunchedEffect(Unit) {
        while (true) {
            delay(1000)
            logText = AppLogger.getRecentLogs(200)
        }
    }

    Dialog(onDismissRequest = onDismiss) {
        Surface(
            modifier = Modifier
                .fillMaxWidth()
                .fillMaxHeight(0.85f),
            shape = RoundedCornerShape(16.dp),
            color = NESColors.shellDark
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = "Debug Log",
                        color = NESColors.textAccent,
                        fontSize = 18.sp,
                        fontWeight = FontWeight.Bold
                    )

                    Row {
                        TextButton(onClick = onShare) {
                            Text("SHARE", color = NESColors.textAccent, fontSize = 12.sp)
                        }
                        TextButton(onClick = onDismiss) {
                            Text("CLOSE", color = NESColors.textSecondary, fontSize = 12.sp)
                        }
                    }
                }

                Spacer(modifier = Modifier.height(12.dp))

                Surface(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxWidth(),
                    color = Color.Black,
                    shape = RoundedCornerShape(8.dp)
                ) {
                    Column(
                        modifier = Modifier
                            .padding(12.dp)
                            .verticalScroll(rememberScrollState())
                    ) {
                        Text(
                            text = logText.ifEmpty { "No logs yet..." },
                            color = Color(0xFF00FF00),
                            fontSize = 9.sp,
                            fontFamily = FontFamily.Monospace,
                            lineHeight = 12.sp
                        )
                    }
                }
            }
        }
    }
}
