package com.nesium

/**
 * JNI interface to the Nesium NES emulator core written in Rust.
 */
object NesiumCore {

    init {
        System.loadLibrary("nesium_android")
    }

    // Button constants matching Rust side
    const val BUTTON_A = 0
    const val BUTTON_B = 1
    const val BUTTON_SELECT = 2
    const val BUTTON_START = 3
    const val BUTTON_RIGHT = 4
    const val BUTTON_LEFT = 5
    const val BUTTON_UP = 6
    const val BUTTON_DOWN = 7

    /**
     * Initialize native logging
     */
    external fun initLogging()

    /**
     * Load a ROM from raw bytes
     * @param romData The ROM file contents
     * @return true if loaded successfully
     */
    external fun loadRomFromBytes(romData: ByteArray): Boolean

    /**
     * Load a ROM from a file path
     * @param path Path to the ROM file
     * @return true if loaded successfully
     */
    external fun loadRomFromPath(path: String): Boolean

    /**
     * Run emulation for one frame
     * @param framebuffer Array to receive ARGB pixel data (256x240 = 61440 ints)
     * @return 1 on success, negative on error
     */
    external fun runFrame(framebuffer: IntArray): Int

    /**
     * Get audio samples from the emulator
     * @param samples Array to receive audio samples
     * @return Number of samples written
     */
    external fun getAudioSamples(samples: FloatArray): Int

    /**
     * Press a button
     * @param button Button ID (see BUTTON_* constants)
     */
    external fun pressButton(button: Int)

    /**
     * Release a button
     * @param button Button ID (see BUTTON_* constants)
     */
    external fun releaseButton(button: Int)

    /**
     * Check if a ROM is currently loaded
     */
    external fun isRomLoaded(): Boolean

    /**
     * Unload the current ROM
     */
    external fun unloadRom()
}
