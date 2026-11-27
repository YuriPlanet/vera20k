package org.vera20k

import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.SystemClock
import android.view.Gravity
import android.view.KeyCharacterMap
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.widget.FrameLayout
import android.widget.TextView
import kotlin.math.abs

/** A compact, optional view overlay; gameplay touch outside buttons still reaches GameActivity. */
internal class TouchHudOverlay(private val activity: VeraActivity) {
    private val density = activity.resources.displayMetrics.density
    private val buttonSize = (46 * density).toInt()
    private val margin = (12 * density).toInt()
    private val preferences = activity.getSharedPreferences(PREFERENCES, 0)
    private val positions = TouchHudLayout.parse(preferences.getString(POSITIONS, null)).toMutableMap()
    private val overlay = FrameLayout(activity).apply {
        clipChildren = false
        clipToPadding = false
        isClickable = false
        isFocusable = false
        setBackgroundColor(Color.TRANSPARENT)
    }
    private val buttons = LinkedHashMap<String, TextView>()
    private var controlsVisible = false
    private var editing = false

    fun attach() {
        addButton("hud", "HUD", "Show touch controls") { toggleControls() }
        addButton("edit", "Edit", "Move touch controls") {
            if (!editing) {
                editing = true
                setVisibility()
            }
        }
        addButton("type", "T", "Select units of the same type") {
            if (!editing) sendKeyStroke(KeyEvent.KEYCODE_T)
        }
        for (group in 1..10) {
            addButton("group-$group", "C$group", "Control group $group: tap to select, hold to assign") {
                if (!editing) sendGroupRecall(group)
            }
            buttons.getValue("group-$group").setOnLongClickListener {
                if (editing) return@setOnLongClickListener false
                sendGroupAssign(group)
                true
            }
        }
        activity.addContentView(
            overlay,
            FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.MATCH_PARENT,
            ),
        )
        overlay.addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ -> positionButtons() }
        setVisibility()
        overlay.post { positionButtons() }
    }

    private fun addButton(id: String, label: String, description: String, click: () -> Unit) {
        val button = TextView(activity).apply {
            text = label
            contentDescription = description
            gravity = Gravity.CENTER
            textSize = if (id == "hud" || id == "edit") 12f else 13f
            setTextColor(Color.WHITE)
            typeface = Typeface.create("sans-serif-medium", Typeface.BOLD)
            background = GradientDrawable().apply {
                shape = GradientDrawable.RECTANGLE
                cornerRadius = 10 * density
                setColor(0xCC14231F.toInt())
                setStroke((1.5f * density).toInt(), 0xCC79C47E.toInt())
            }
            elevation = 6 * density
            isClickable = true
            isFocusable = false
            isFocusableInTouchMode = false
            setOnClickListener { click() }
            setOnTouchListener { view, event -> dragButton(id, view, event) }
        }
        buttons[id] = button
        overlay.addView(button, FrameLayout.LayoutParams(buttonSize, buttonSize))
    }

    private fun toggleControls() {
        controlsVisible = !controlsVisible
        if (!controlsVisible) editing = false
        setVisibility()
    }

    private fun setVisibility() {
        buttons.forEach { (id, button) ->
            button.visibility = when {
                id == "hud" -> View.VISIBLE
                controlsVisible -> View.VISIBLE
                else -> View.GONE
            }
            button.alpha = if (editing && id != "edit") 0.62f else 1f
        }
        buttons["edit"]?.apply {
            text = if (editing) "Done" else "Edit"
            contentDescription = if (editing) "Finish moving touch controls" else "Move touch controls"
        }
    }

    private fun dragButton(id: String, view: View, event: MotionEvent): Boolean {
        if (!editing) return false
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                view.tag = DragOrigin(event.rawX, event.rawY, view.x, view.y)
                return true
            }
            MotionEvent.ACTION_MOVE -> {
                val origin = view.tag as? DragOrigin ?: return true
                view.x = (origin.viewX + event.rawX - origin.rawX)
                    .coerceIn(0f, (overlay.width - view.width).coerceAtLeast(0).toFloat())
                view.y = (origin.viewY + event.rawY - origin.rawY)
                    .coerceIn(0f, (overlay.height - view.height).coerceAtLeast(0).toFloat())
                return true
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                val origin = view.tag as? DragOrigin
                view.tag = null
                if (event.actionMasked == MotionEvent.ACTION_UP && origin != null &&
                    abs(event.rawX - origin.rawX) < touchSlop &&
                    abs(event.rawY - origin.rawY) < touchSlop && id == "edit"
                ) {
                    editing = false
                    setVisibility()
                }
                savePosition(id, view)
                return true
            }
        }
        return true
    }

    private fun savePosition(id: String, view: View) {
        val maxX = (overlay.width - view.width).coerceAtLeast(1)
        val maxY = (overlay.height - view.height).coerceAtLeast(1)
        positions[id] = TouchHudPoint(view.x / maxX, view.y / maxY).bounded()
        preferences.edit().putString(POSITIONS, TouchHudLayout.serialize(positions)).apply()
    }

    private fun positionButtons() {
        if (overlay.width <= 0 || overlay.height <= 0) return
        val maxX = (overlay.width - buttonSize).coerceAtLeast(0)
        val maxY = (overlay.height - buttonSize).coerceAtLeast(0)
        val defaults = defaultPositions(maxX, maxY)
        for ((id, button) in buttons) {
            if (editing && button.tag is DragOrigin) continue
            val point = positions[id] ?: defaults.getValue(id)
            button.x = (point.x * maxX).coerceIn(0f, maxX.toFloat())
            button.y = (point.y * maxY).coerceIn(0f, maxY.toFloat())
        }
    }

    private fun defaultPositions(maxX: Int, maxY: Int): Map<String, TouchHudPoint> {
        fun point(x: Float, y: Float) = TouchHudPoint(
            (margin + (maxX - 2 * margin).coerceAtLeast(0) * x) / maxX.coerceAtLeast(1),
            (margin + (maxY - 2 * margin).coerceAtLeast(0) * y) / maxY.coerceAtLeast(1),
        ).bounded()
        val result = linkedMapOf(
            "hud" to point(0.05f, 0.94f),
            "edit" to point(0.25f, 0.94f),
            "type" to point(0.45f, 0.94f),
        )
        for (group in 1..10) {
            val column = (group - 1) / 5
            val row = (group - 1) % 5
            result["group-$group"] = point(0.74f + column * 0.22f, 0.08f + row * 0.20f)
        }
        return result
    }

    private fun sendGroupRecall(group: Int) {
        sendKeyStroke(if (group == 10) KeyEvent.KEYCODE_0 else KeyEvent.KEYCODE_1 + group - 1)
    }

    private fun sendGroupAssign(group: Int) {
        sendKeyStroke(
            if (group == 10) KeyEvent.KEYCODE_0 else KeyEvent.KEYCODE_1 + group - 1,
            KeyEvent.META_CTRL_ON or KeyEvent.META_CTRL_LEFT_ON,
            controlChord = true,
        )
    }

    /** GameActivity forwards these key edges to the same native input queue as hardware keys. */
    private fun sendKeyStroke(keyCode: Int, metaState: Int = 0, controlChord: Boolean = false) {
        val now = SystemClock.uptimeMillis()
        if (controlChord) dispatchKey(KeyEvent.KEYCODE_CTRL_LEFT, KeyEvent.ACTION_DOWN, now, now, 0)
        dispatchKey(keyCode, KeyEvent.ACTION_DOWN, now, now, metaState)
        dispatchKey(keyCode, KeyEvent.ACTION_UP, now, now + 1, metaState)
        if (controlChord) dispatchKey(KeyEvent.KEYCODE_CTRL_LEFT, KeyEvent.ACTION_UP, now, now + 2, 0)
    }

    private fun dispatchKey(keyCode: Int, action: Int, downTime: Long, eventTime: Long, metaState: Int) {
        val event = KeyEvent(
            downTime,
            eventTime,
            action,
            keyCode,
            0,
            metaState,
            KeyCharacterMap.VIRTUAL_KEYBOARD,
            0,
            KeyEvent.FLAG_SOFT_KEYBOARD or KeyEvent.FLAG_KEEP_TOUCH_MODE,
        )
        try {
            if (action == KeyEvent.ACTION_DOWN) activity.forwardHudKeyDown(keyCode, event)
            else activity.forwardHudKeyUp(keyCode, event)
        } finally {
            event.recycle()
        }
    }

    private data class DragOrigin(val rawX: Float, val rawY: Float, val viewX: Float, val viewY: Float)

    private val touchSlop = (8 * density)

    private companion object {
        const val PREFERENCES = "touch_hud"
        const val POSITIONS = "button_positions_v1"
    }
}
