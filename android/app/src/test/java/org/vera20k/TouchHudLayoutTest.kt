package org.vera20k

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TouchHudLayoutTest {
    @Test
    fun roundTripKeepsKnownButtonPositions() {
        val positions = mapOf(
            "hud" to TouchHudPoint(0.91f, 0.82f),
            "type" to TouchHudPoint(0.24f, 0.68f),
            "group-10" to TouchHudPoint(0.73f, 0.32f),
        )

        assertEquals(positions, TouchHudLayout.parse(TouchHudLayout.serialize(positions)))
    }

    @Test
    fun parseClampsCoordinatesAndIgnoresUnknownOrMalformedEntries() {
        val parsed = TouchHudLayout.parse("hud,1.4,-0.2;unknown,0.5,0.5;edit,NaN,0.5;type,0.4,0.6")

        assertEquals(TouchHudPoint(1f, 0f), parsed["hud"])
        assertEquals(TouchHudPoint(0.4f, 0.6f), parsed["type"])
        assertFalse(parsed.containsKey("unknown"))
        assertFalse(parsed.containsKey("edit"))
    }

    @Test
    fun idsCoverTheVisibleHudAndExcludeUnrecognizedButtons() {
        assertTrue(TouchHudLayout.buttonIds.contains("hud"))
        assertTrue(TouchHudLayout.buttonIds.contains("group-1"))
        assertTrue(TouchHudLayout.buttonIds.contains("group-10"))
        assertFalse(TouchHudLayout.buttonIds.contains("group-11"))
    }
}
