extends Node2D

# Audio-reactive caption scaling variables
var audio_capture: AudioEffectCapture = null
var video_audio_bus_index: int = -1
var caption_label_ref: RichTextLabel = null
var base_caption_scale: Vector2 = Vector2.ONE
var current_scale_multiplier: float = 1.0
var target_scale_multiplier: float = 1.0
const SCALE_LERP_SPEED: float = 10.0  # Higher = more responsive/snappier
const MIN_SCALE: float = 1.0 # Quiet audio
const MAX_SCALE: float = 1.08  # Loud audio
const AMPLITUDE_THRESHOLD: float = 0.3
var debug_frame_count: int = 0  # For debug output throttling

# Audio-reactive video stretching variables
var video_player_ref: VideoStreamPlayer = null
var base_video_scale: Vector2 = Vector2.ONE
var base_video_position: Vector2 = Vector2.ZERO
var video_width: float = 0.0  # Store video display width for centering
var current_video_scale_multiplier: float = 1.0
var target_video_scale_multiplier: float = 1.0
const VIDEO_MIN_SCALE: float = 1.0  # Normal size when quiet
const VIDEO_MAX_SCALE: float = 1.05  # 8% wider when loud

func _ready():
	# Set viewport background to transparent
	get_tree().root.get_viewport().transparent_bg = true

	# Move to second screen if available
	move_to_second_screen()

	# Setup audio capture for audio-reactive captions
	setup_audio_capture()

	var args = OS.get_cmdline_args()
	print("Command line args: ", args)
	if args.size() > 0:
		var file_path = args[0]
		var media_type = "video"  # default to video
		var caption_text = ""
		var sender_name = ""
		var width = 1920
		var height = 1080

		# Parse arguments: <file_path> [media_type] [caption] [sender] [width] [height]
		if args.size() > 1:
			# Check if arg[1] is media type or caption
			if args[1] == "image" or args[1] == "video":
				media_type = args[1]
				if args.size() > 2:
					caption_text = args[2]
				if args.size() > 3:
					sender_name = args[3]
				if args.size() > 5:
					width = args[4].to_int()
					height = args[5].to_int()
			else:
				# Old format: arg[1] is caption
				caption_text = args[1]
				if args.size() > 2:
					sender_name = args[2]
				if args.size() > 4:
					width = args[3].to_int()
					height = args[4].to_int()

		if media_type == "image":
			play_image(file_path, caption_text, sender_name, width, height)
		else:
			play_video(file_path, caption_text, sender_name, width, height)
	else:
		push_error("No media file provided")
		OS.kill(OS.get_process_id())

func play_video(video_path: String, caption_text: String, sender_name: String, actual_width: int, actual_height: int):
	print("Attempting to play video: ", video_path)
	if not FileAccess.file_exists(video_path):
		push_error("File doesn't exist: ", video_path)
		OS.kill(OS.get_process_id())
		return

	# Create VideoStreamPlayer
	var video_player = VideoStreamPlayer.new()
	video_player.name = "VideoPlayer"
	video_player.bus = "VideoAudio"  # Route audio through our custom bus for capture
	add_child(video_player)

	var video_stream = load(video_path)
	print("Loaded video stream: ", video_stream)

	if video_stream:
		video_player.stream = video_stream
		video_player.play()

		# Wait for video to load
		await RenderingServer.frame_post_draw
		await get_tree().process_frame

		print("Video dimensions: ", actual_width, "x", actual_height)
		
		# Define maximum window size
		var max_width = 1280
		var max_height = 720
		
		# Calculate scale to fit within max dimensions while maintaining aspect ratio
		var scale_x = float(max_width) / float(actual_width)
		var scale_y = float(max_height) / float(actual_height)
		var final_scale = min(min(scale_x, scale_y), 1.0)  # Don't upscale, only downscale
		
		# Apply scaling to video player
		video_player.scale = Vector2(final_scale, final_scale)

		# Store reference and base scale for audio-reactive stretching
		video_player_ref = video_player
		base_video_scale = video_player.scale
		
		# Center the video in the container
		video_player.position = Vector2(
			(float(max_width) - float(actual_width) * final_scale) / 2.0,
			(float(max_height) - float(actual_height) * final_scale) / 2.0
		)

		# Store base position and width for centered stretching
		base_video_position = video_player.position
		video_width = float(actual_width) * final_scale
		
		# Create caption label
		if caption_text != "":
			var caption_label = RichTextLabel.new()
			caption_label.name = "CaptionLabel"
			caption_label.bbcode_enabled = true
			caption_label.add_theme_constant_override("outline_size", 5)
			caption_label.add_theme_color_override("font_outline_color", Color(0, 0, 0, 1))  # Black outline
			caption_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART  # Wraps at word boundaries and breaks long words
			caption_label.fit_content = true  # Fit content height

			# Load custom font
			var font = load("res://fonts/impact.ttf")
			if font:
				caption_label.add_theme_font_override("normal_font", font)
				caption_label.add_theme_font_override("bold_font", font)
				caption_label.add_theme_font_override("italics_font", font)
				caption_label.add_theme_font_override("bold_italics_font", font)

			# Calculate video display dimensions
			var video_display_width = float(actual_width) * final_scale
			var video_display_height = float(actual_height) * final_scale

			# Set label size to allow overflow (use max width instead of video width)
			caption_label.size = Vector2(float(max_width), 150)  # Increased height for overflow

			# Set pivot to center for rotation (prevents up/down movement during rotation)
			caption_label.pivot_offset = caption_label.size / 2.0

			# Position below the video (not overlapping)
			caption_label.position = Vector2(
				(float(max_width) - float(max_width)) / 2.0,  # Center in window
				video_player.position.y + video_display_height + 10.0  # 10 pixels below video bottom
			)
			
			if caption_label.position.y > 600:
				caption_label.position.y = 600
			
			# Debug information
			print("Video display size: ", video_display_width, "x", video_display_height)
			print("Video position: ", video_player.position)
			print("Label position: ", caption_label.position)
			print("Label size: ", caption_label.size)
			print("Caption text: ", caption_text)

			# Store plain caption text for animation
			var plain_caption = caption_text
			var font_size = 48  # Increased font size for better visibility
			caption_label.text = "[center][font_size=" + str(font_size) + "]" + caption_text + "[/font_size][/center]"

			# Add a small delay to ensure text rendering
			await get_tree().create_timer(0.1).timeout
			add_child(caption_label)

			# Animate the caption
			animate_caption_audio_reactive(caption_label, plain_caption, 5.0)

		# Create sender label in top-left
		if sender_name != "":
			var sender_label = RichTextLabel.new()
			sender_label.name = "SenderLabel"
			sender_label.bbcode_enabled = true
			sender_label.add_theme_constant_override("outline_size", 5)
			sender_label.add_theme_color_override("font_outline_color", Color(0, 0, 0, 1))  # Black outline
			sender_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
			sender_label.fit_content = true

			# Load custom font
			var font = load("res://fonts/impact.ttf")
			if font:
				sender_label.add_theme_font_override("normal_font", font)
				sender_label.add_theme_font_override("bold_font", font)
				sender_label.add_theme_font_override("italics_font", font)
				sender_label.add_theme_font_override("bold_italics_font", font)

			# Set label size
			sender_label.size = Vector2(400, 80)  # Smaller than caption

			# Position in top-left of video (no pivot offset for small labels)
			sender_label.position = Vector2(
				video_player.position.x + 10.0,  # 10 pixels from left edge of video
				video_player.position.y + 10.0   # 10 pixels from top edge of video
			)

			# Store plain sender text for animation
			var plain_sender = sender_name
			var sender_font_size = 32  # Smaller font for sender
			sender_label.text = "[font_size=" + str(sender_font_size) + "]" + sender_name + "[/font_size]"

			# Add sender label
			await get_tree().create_timer(0.1).timeout
			add_child(sender_label)

			# Animate the sender label
			animate_caption(sender_label, plain_sender, 5.0)

		print("Max window size: ", max_width, "x", max_height)
		print("Final scale: ", final_scale)
		print("Scaled size: ", float(actual_width) * final_scale, "x", float(actual_height) * final_scale)
		print("Video position: ", video_player.position)
		
		video_player.finished.connect(func(): 
			print("Video finished, exiting")
			OS.kill(OS.get_process_id())
		)
	else:
		push_error("Failed to load video")
		OS.kill(OS.get_process_id())

func play_image(image_path: String, caption_text: String, sender_name: String, actual_width: int, actual_height: int):
	print("Attempting to display image: ", image_path)
	if not FileAccess.file_exists(image_path):
		push_error("File doesn't exist: ", image_path)
		OS.kill(OS.get_process_id())
		return

	# Load image
	var image = Image.load_from_file(image_path)
	if not image:
		push_error("Failed to load image")
		OS.kill(OS.get_process_id())
		return

	var texture = ImageTexture.create_from_image(image)
	print("Loaded image texture: ", texture)

	# Create TextureRect to display image
	var texture_rect = TextureRect.new()
	texture_rect.name = "ImageDisplay"
	texture_rect.texture = texture
	texture_rect.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	texture_rect.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT
	add_child(texture_rect)

	print("Image dimensions: ", actual_width, "x", actual_height)

	# Define maximum window size
	var max_width = 1280
	var max_height = 720

	# Calculate scale to fit within max dimensions while maintaining aspect ratio
	var scale_x = float(max_width) / float(actual_width)
	var scale_y = float(max_height) / float(actual_height)
	var final_scale = min(min(scale_x, scale_y), 1.0)  # Don't upscale, only downscale

	# Calculate display size
	var display_width = float(actual_width) * final_scale
	var display_height = float(actual_height) * final_scale

	# Set texture rect size and position
	texture_rect.size = Vector2(display_width, display_height)
	texture_rect.position = Vector2(
		(float(max_width) - display_width) / 2.0,
		(float(max_height) - display_height) / 2.0
	)

	# Create caption label
	if caption_text != "":
		var caption_label = RichTextLabel.new()
		caption_label.name = "CaptionLabel"
		caption_label.bbcode_enabled = true
		caption_label.add_theme_constant_override("outline_size", 5)
		caption_label.add_theme_color_override("font_outline_color", Color(0, 0, 0, 1))  # Black outline
		caption_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART  # Wraps at word boundaries and breaks long words
		caption_label.fit_content = true  # Fit content height

		# Load custom font
		var font = load("res://fonts/impact.ttf")
		if font:
			caption_label.add_theme_font_override("normal_font", font)
			caption_label.add_theme_font_override("bold_font", font)
			caption_label.add_theme_font_override("italics_font", font)
			caption_label.add_theme_font_override("bold_italics_font", font)

		# Set label size
		caption_label.size = Vector2(float(max_width), 150)

		# Set pivot to center for rotation (prevents up/down movement during rotation)
		caption_label.pivot_offset = caption_label.size / 2.0

		# Position below the image
		caption_label.position = Vector2(
			(float(max_width) - float(max_width)) / 2.0,  # Center in window
			texture_rect.position.y + display_height + 10.0  # 10 pixels below image bottom
		)

		if caption_label.position.y > 600:
			caption_label.position.y = 600

		# Debug information
		print("Image display size: ", display_width, "x", display_height)
		print("Image position: ", texture_rect.position)
		print("Label position: ", caption_label.position)
		print("Label size: ", caption_label.size)
		print("Caption text: ", caption_text)

		# Store plain caption text for animation
		var plain_caption = caption_text
		var font_size = 48  # Font size for visibility
		caption_label.text = "[center][font_size=" + str(font_size) + "]" + caption_text + "[/font_size][/center]"

		# Add a small delay to ensure text rendering
		await get_tree().create_timer(0.1).timeout
		add_child(caption_label)

		# Animate the caption 
		animate_caption(caption_label, plain_caption, 5.0)

	# Create sender label in top-left
	if sender_name != "":
		var sender_label = RichTextLabel.new()
		sender_label.name = "SenderLabel"
		sender_label.bbcode_enabled = true
		sender_label.add_theme_constant_override("outline_size", 5)
		sender_label.add_theme_color_override("font_outline_color", Color(0, 0, 0, 1))  # Black outline
		sender_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		sender_label.fit_content = true

		# Load custom font
		var font = load("res://fonts/impact.ttf")
		if font:
			sender_label.add_theme_font_override("normal_font", font)
			sender_label.add_theme_font_override("bold_font", font)
			sender_label.add_theme_font_override("italics_font", font)
			sender_label.add_theme_font_override("bold_italics_font", font)

		# Set label size
		sender_label.size = Vector2(400, 80)  # Smaller than caption

		# Position in top-left of image (no pivot offset for small labels)
		sender_label.position = Vector2(
			texture_rect.position.x + 10.0,  # 10 pixels from left edge of image
			texture_rect.position.y + 10.0   # 10 pixels from top edge of image
		)

		# Store plain sender text for animation
		var plain_sender = sender_name
		var sender_font_size = 32  # Smaller font for sender
		sender_label.text = "[font_size=" + str(sender_font_size) + "]" + sender_name + "[/font_size]"

		# Add sender label
		await get_tree().create_timer(0.1).timeout
		add_child(sender_label)

		# Animate the sender label
		animate_caption(sender_label, plain_sender, 5.0)

	print("Max window size: ", max_width, "x", max_height)
	print("Final scale: ", final_scale)
	print("Scaled size: ", display_width, "x", display_height)
	print("Image position: ", texture_rect.position)

	# Set timer to close after 3 seconds
	await get_tree().create_timer(3.0).timeout
	print("Image display timeout, exiting")
	OS.kill(OS.get_process_id())

func animate_caption_audio_reactive(caption_label: RichTextLabel, caption_text: String, duration: float = 5.0):
	"""
	Animates caption with slow looping tilt effect AND audio-reactive scaling.
	The rotation animation is handled here, while scaling is handled in _process().

	Args:
		caption_label: The RichTextLabel to animate
		caption_text: The plain text to display (without BBCode)
		duration: Animation duration in seconds for one full cycle
	"""
	# Store reference for audio-reactive scaling in _process()
	caption_label_ref = caption_label
	base_caption_scale = caption_label.scale

	# Set initial state - show all text immediately
	caption_label.visible_characters = -1  # -1 means show all characters
	caption_label.rotation = -0.08  # Start at LEFT position (not center)

	# Ensure pivot is centered for proper scaling
	caption_label.pivot_offset = caption_label.size / 2.0

	# Create tween for rotation animation
	var rotation_tween = create_tween()
	rotation_tween.set_loops()  # Loop infinitely
	rotation_tween.set_parallel(false)  # Sequential rotations

	# Swing from left to right
	rotation_tween.tween_property(
		caption_label,
		"rotation",
		0.08,  # Tilt right (~4.6 degrees)
		duration / 2.0
	).set_ease(Tween.EASE_IN_OUT).set_trans(Tween.TRANS_SINE)

	# Swing from right back to left
	rotation_tween.tween_property(
		caption_label,
		"rotation",
		-0.08,  # Tilt left (~-4.6 degrees)
		duration / 2.0
	).set_ease(Tween.EASE_IN_OUT).set_trans(Tween.TRANS_SINE)

func animate_caption(caption_label: RichTextLabel, caption_text: String, duration: float = 5.0):
	"""
	Animates caption with slow looping tilt effect (without audio reactivity).
	Used for sender labels and non-audio-reactive text.

	Args:
		caption_label: The RichTextLabel to animate
		caption_text: The plain text to display (without BBCode)
		duration: Animation duration in seconds for one full cycle
	"""
	# Set initial state - show all text immediately
	caption_label.visible_characters = -1  # -1 means show all characters
	caption_label.rotation = -0.08  # Start at LEFT position (not center)

	# Create tween for rotation animation
	var rotation_tween = create_tween()
	rotation_tween.set_loops()  # Loop infinitely
	rotation_tween.set_parallel(false)  # Sequential rotations

	# Swing from left to right
	rotation_tween.tween_property(
		caption_label,
		"rotation",
		0.08,  # Tilt right (~4.6 degrees)
		duration / 2.0
	).set_ease(Tween.EASE_IN_OUT).set_trans(Tween.TRANS_SINE)

	# Swing from right back to left
	rotation_tween.tween_property(
		caption_label,
		"rotation",
		-0.08,  # Tilt left (~-4.6 degrees)
		duration / 2.0
	).set_ease(Tween.EASE_IN_OUT).set_trans(Tween.TRANS_SINE)

func move_to_second_screen() -> void:
	"""
	Moves the window to the second screen if available.
	If only one screen exists, stays on the primary screen.
	"""
	var screen_count = DisplayServer.get_screen_count()
	print("Detected screens: ", screen_count)

	if screen_count >= 2:
		# Move to second screen (index 1)
		DisplayServer.window_set_current_screen(1)
		print("Moved window to screen 1 (second monitor)")

		# Optional: Center window on the second screen
		var screen_position = DisplayServer.screen_get_position(1)
		var screen_size = DisplayServer.screen_get_size(1)
		var window_size = DisplayServer.window_get_size()

		var centered_position = Vector2i(
			screen_position.x + (screen_size.x - window_size.x) / 2,
			screen_position.y + (screen_size.y - window_size.y) / 2
		)

		DisplayServer.window_set_position(centered_position)
		print("Window centered at: ", centered_position)
	else:
		print("Only one screen detected, staying on primary screen")

func setup_audio_capture() -> void:
	"""
	Creates a custom audio bus for video audio and sets up AudioEffectCapture
	for real-time audio amplitude analysis.
	"""
	# Create a new audio bus for video audio
	var bus_count = AudioServer.bus_count
	AudioServer.add_bus(bus_count)
	AudioServer.set_bus_name(bus_count, "VideoAudio")
	AudioServer.set_bus_send(bus_count, "Master")
	video_audio_bus_index = bus_count

	# Create and configure AudioEffectCapture
	audio_capture = AudioEffectCapture.new()
	audio_capture.buffer_length = 0.1  # 30ms buffer for lower latency

	# Add the effect to the bus
	AudioServer.add_bus_effect(video_audio_bus_index, audio_capture, 0)

	print("Audio capture initialized on bus 'VideoAudio' (index: ", video_audio_bus_index, ")")

func sample_audio_amplitude() -> float:
	"""
	Samples the audio buffer and calculates RMS (Root Mean Square) amplitude.
	Returns a normalized value between 0.0 (silence) and 1.0 (loud).
	"""
	if audio_capture == null:
		return 0.0

	var frames_available = audio_capture.get_frames_available()
	if frames_available <= 0:
		return 0.0

	# Sample up to 50 frames for analysis (faster, more responsive)
	var frames_to_sample = min(50, frames_available)

	if not audio_capture.can_get_buffer(frames_to_sample):
		return 0.0

	var buffer = audio_capture.get_buffer(frames_to_sample)
	if buffer.size() == 0:
		return 0.0

	# Calculate RMS amplitude
	var sum_squared: float = 0.0
	for frame in buffer:
		# Convert stereo to mono by averaging channels
		var mono = (frame.x + frame.y) / 2.0
		sum_squared += mono * mono

	# Calculate RMS
	var rms = sqrt(sum_squared / float(buffer.size()))

	# Normalize and map to 0.0-1.0 range
	# Multiply by 3.0 to boost sensitivity (adjust if needed)
	var normalized = clamp(rms * 3.0, 0.0, 1.0)

	return normalized

func _process(delta: float) -> void:
	"""
	Called every frame. Applies audio-reactive scaling to the caption label.
	"""
	if caption_label_ref == null:
		return

	# Sample audio amplitude
	var amplitude = sample_audio_amplitude()

	# Apply threshold to filter out very quiet audio/noise
	if amplitude < AMPLITUDE_THRESHOLD:
		amplitude = 0.0

	# Map amplitude to scale range (MIN_SCALE to MAX_SCALE)
	target_scale_multiplier = lerp(MIN_SCALE, MAX_SCALE, amplitude)

	# Smooth transition using lerp to prevent jitter
	current_scale_multiplier = lerp(
		current_scale_multiplier,
		target_scale_multiplier,
		SCALE_LERP_SPEED * delta
	)

	# Apply the scale to the caption label
	caption_label_ref.scale = base_caption_scale * current_scale_multiplier

	# Apply audio-reactive horizontal stretching to video
	if video_player_ref != null:
		# Map amplitude to video scale range (VIDEO_MIN_SCALE to VIDEO_MAX_SCALE)
		target_video_scale_multiplier = lerp(VIDEO_MIN_SCALE, VIDEO_MAX_SCALE, amplitude)

		# Smooth transition
		current_video_scale_multiplier = lerp(
			current_video_scale_multiplier,
			target_video_scale_multiplier,
			SCALE_LERP_SPEED * delta
		)

		# Apply horizontal stretch (scale x-axis only)
		video_player_ref.scale.x = base_video_scale.x * current_video_scale_multiplier
		# Keep y-axis at base scale to only stretch horizontally
		video_player_ref.scale.y = base_video_scale.y

		# Adjust position to keep video centered (stretch left and right equally)
		var scale_offset = current_video_scale_multiplier - 1.0
		var position_offset = -(video_width * scale_offset) / 2.0
		video_player_ref.position.x = base_video_position.x + position_offset
		# Keep y position unchanged
		video_player_ref.position.y = base_video_position.y

	# Debug output (every 30 frames ≈ twice per second at 60fps)
	debug_frame_count += 1
	if debug_frame_count >= 30:
		debug_frame_count = 0
		print("Audio: amp=%.3f, caption_scale=%.3f, video_scale=%.3f" % [amplitude, current_scale_multiplier, current_video_scale_multiplier])

func _exit_tree() -> void:
	"""
	Cleanup when the node is removed from the scene tree.
	Clears references to prevent memory leaks.
	"""
	audio_capture = null
	caption_label_ref = null
	video_player_ref = null
