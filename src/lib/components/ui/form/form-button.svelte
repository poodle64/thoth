<script lang="ts">
	import { Button, type ButtonProps } from "@poodle64/ui/button";

	let { ref = $bindable(null), ...restProps }: ButtonProps = $props();
</script>

<Button bind:ref type="submit" {...restProps} />
