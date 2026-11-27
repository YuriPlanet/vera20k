package org.vera20k

/** A button position as a fraction of the usable game window. */
internal data class TouchHudPoint(val x: Float, val y: Float) {
    fun bounded(): TouchHudPoint = TouchHudPoint(x.coerceIn(0f, 1f), y.coerceIn(0f, 1f))
}

/** Small, version-tolerant persistence format for movable Android HUD buttons. */
internal object TouchHudLayout {
    val buttonIds: List<String> = listOf("hud", "edit", "type") +
        (1..10).map { "group-$it" }

    fun parse(serialized: String?): Map<String, TouchHudPoint> {
        if (serialized.isNullOrBlank()) return emptyMap()
        val positions = LinkedHashMap<String, TouchHudPoint>()
        for (entry in serialized.split(';')) {
            val fields = entry.split(',')
            if (fields.size != 3 || fields[0] !in buttonIds) continue
            val x = fields[1].toFloatOrNull() ?: continue
            val y = fields[2].toFloatOrNull() ?: continue
            if (!x.isFinite() || !y.isFinite()) continue
            positions[fields[0]] = TouchHudPoint(x, y).bounded()
        }
        return positions
    }

    fun serialize(positions: Map<String, TouchHudPoint>): String = buttonIds
        .mapNotNull { id -> positions[id]?.bounded()?.let { "$id,${it.x},${it.y}" } }
        .joinToString(";")
}
