//! Feedback text for commands.
//!
//! Feedback carries vanilla's translation keys and arguments; the protocol only has a plain
//! text component so the English 26.3 `en_us` strings are rendered here. Keeping the keyed
//! lookup means a future translatable-component packet only needs to send the key and args.

/// The vanilla `en_us` format string for a key.
fn template(key: &str) -> Option<&'static str> {
    Some(match key {
        "commands.rotate.success" => "Rotated %s",
        "commands.fillbiome.success.count" => {
            "%s biome entries set between %s, %s, %s and %s, %s, %s"
        }
        "commands.fillbiome.no_changes" => "No biomes were changed",
        "commands.fillbiome.toobig" => {
            "Too many blocks in the specified area (maximum %s, specified %s)"
        }
        "chat.type.advancement.task" => "%s has made the advancement %s",
        "chat.type.advancement.goal" => "%s has reached the goal %s",
        "chat.type.advancement.challenge" => "%s has completed the challenge %s",
        "commands.debug.function.noRecursion" => "Can't trace from inside of function",
        "commands.debug.function.noReturnRun" => "Tracing can't be used with return run",
        "commands.debug.function.success.single" => {
            "Traced %s command(s) from function '%s' to output file %s"
        }
        "commands.debug.function.traceFailed" => "Failed to trace function",
        "argument.pos.unloaded" => "That position is not loaded",
        "commands.advancement.grant.one.to.one.success" => "Granted the advancement %s to %s",
        "commands.advancement.grant.one.to.many.success" => {
            "Granted the advancement %s to %s players"
        }
        "commands.advancement.grant.many.to.one.success" => "Granted %s advancements to %s",
        "commands.advancement.grant.many.to.many.success" => {
            "Granted %s advancements to %s players"
        }
        "commands.advancement.grant.criterion.to.one.success" => {
            "Granted the criterion '%s' of advancement %s to %s"
        }
        "commands.advancement.grant.criterion.to.many.success" => {
            "Granted the criterion '%s' of advancement %s to %s players"
        }
        "commands.advancement.grant.one.to.one.failure" => {
            "Couldn't grant advancement %s to %s as nothing changed"
        }
        "commands.advancement.grant.one.to.many.failure" => {
            "Couldn't grant advancement %s to %s players as nothing changed"
        }
        "commands.advancement.grant.many.to.one.failure" => {
            "Couldn't grant %s advancements to %s as nothing changed"
        }
        "commands.advancement.grant.many.to.many.failure" => {
            "Couldn't grant %s advancements to %s players as nothing changed"
        }
        "commands.advancement.grant.criterion.to.one.failure" => {
            "Couldn't grant criterion '%s' of advancement %s to %s as nothing changed"
        }
        "commands.advancement.grant.criterion.to.many.failure" => {
            "Couldn't grant criterion '%s' of advancement %s to %s players as nothing changed"
        }
        "commands.advancement.revoke.one.to.one.success" => "Revoked the advancement %s from %s",
        "commands.advancement.revoke.one.to.many.success" => {
            "Revoked the advancement %s from %s players"
        }
        "commands.advancement.revoke.many.to.one.success" => "Revoked %s advancements from %s",
        "commands.advancement.revoke.many.to.many.success" => {
            "Revoked %s advancements from %s players"
        }
        "commands.advancement.revoke.criterion.to.one.success" => {
            "Revoked the criterion '%s' of advancement %s from %s"
        }
        "commands.advancement.revoke.criterion.to.many.success" => {
            "Revoked the criterion '%s' of advancement %s from %s players"
        }
        "commands.advancement.revoke.one.to.one.failure" => {
            "Couldn't revoke advancement %s from %s as nothing changed"
        }
        "commands.advancement.revoke.one.to.many.failure" => {
            "Couldn't revoke advancement %s from %s players as nothing changed"
        }
        "commands.advancement.revoke.many.to.one.failure" => {
            "Couldn't revoke %s advancements from %s as nothing changed"
        }
        "commands.advancement.revoke.many.to.many.failure" => {
            "Couldn't revoke %s advancements from %s players as nothing changed"
        }
        "commands.advancement.revoke.criterion.to.one.failure" => {
            "Couldn't revoke criterion '%s' of advancement %s from %s as nothing changed"
        }
        "commands.advancement.revoke.criterion.to.many.failure" => {
            "Couldn't revoke criterion '%s' of advancement %s from %s players as nothing changed"
        }
        "advancement.advancementNotFound" => "Unknown advancement: %s",
        "commands.advancement.criterionNotFound" => {
            "The advancement %s does not contain the criterion '%s'"
        }
        "recipe.notFound" => "Unknown recipe: %s",
        "commands.recipe.give.success.single" => "Successfully gave %s recipes to %s",
        "commands.recipe.give.success.multiple" => "Successfully gave %s recipes to %s players",
        "commands.recipe.take.success.single" => "Successfully took %s recipes from %s",
        "commands.recipe.take.success.multiple" => "Successfully took %s recipes from %s players",
        "commands.recipe.give.failed" => "Could not learn any new recipes",
        "commands.recipe.take.failed" => "Could not forget any recipes",
        "commands.compute.success" => "Computed %s: %s",
        "commands.compute.failed" => "Could not compute %s",
        "argument.id.unknown" => "Unknown ID: %s",
        "commands.waypoint.list.empty" => "No waypoints found",
        "commands.waypoint.list.success" => "%s waypoints: %s",
        "commands.waypoint.modify.color" => "Changed waypoint color of %s to %s",
        "commands.waypoint.modify.color.reset" => "Reset waypoint color of %s",
        "commands.waypoint.modify.style" => "Changed waypoint style of %s",
        "commands.ride.not_riding" => "%s is not riding any vehicle",
        "commands.drop.no_loot_table" => "Entity %s has no loot table",
        "commands.item.entity.set.success.single" => "Replaced a slot on %s with %s",
        "commands.item.entity.set.success.multiple" => "Replaced a slot on %s entities with %s",
        "slot.unknown" => "Unknown slot '%s'",
        "commands.item.source.not_a_container" => "Source position %s, %s, %s is not a container",
        "commands.item.target.not_a_container" => "Target position %s, %s, %s is not a container",
        "commands.item.source.no_such_slot" => "The source does not have slot %s",
        "commands.item.target.no_such_slot" => "The target does not have slot %s",
        "commands.item.target.no_changes" => "No targets accepted item into slot %s",
        "commands.item.block.set.success" => "Replaced a slot at %s, %s, %s with %s",

        "commands.place.template.invalid" => "There is no template with ID \"%s\"",
        "commands.ride.dismount.success" => "%s stopped riding %s",
        "commands.ride.mount.success" => "%s started riding %s",
        "commands.ride.already_riding" => "%s is already riding %s",
        "commands.ride.mount.failure.generic" => "%s could not start riding %s",
        "commands.ride.mount.failure.cant_ride_players" => "Players can't be ridden",
        "commands.ride.mount.failure.loop" => "Can't mount an entity on itself or its passengers",
        "commands.drop.success.single" => "Dropped %s %s",
        "commands.drop.success.multiple" => "Dropped %s items",
        "argument.entity.notfound.entity" => "No entity was found",
        "argument.item_modifier.unknown" => "Unknown item modifier '%s'",
        "commands.swing.success.single" => "Swung %s's arm",
        "commands.swing.success.multiple" => "Swung the arms of %s entities",
        "commands.attribute.value.get.success" => "Value of attribute %s for %s is %s",
        "commands.attribute.base_value.get.success" => "Base value of attribute %s for %s is %s",
        "commands.attribute.base_value.set.success" => {
            "Base value for attribute %s for %s set to %s"
        }
        "commands.attribute.base_value.reset.success" => {
            "Base value for attribute %s for %s reset to default %s"
        }
        "commands.attribute.modifier.add.success" => "Added modifier %s to attribute %s for %s",
        "commands.attribute.modifier.remove.success" => {
            "Removed modifier %s from attribute %s for %s"
        }
        "commands.attribute.modifier.value.get.success" => {
            "Value of modifier %s on attribute %s for %s is %s"
        }
        "commands.attribute.failed.no_attribute" => "Entity %s has no attribute %s",
        "commands.attribute.failed.modifier_already_present" => {
            "Modifier %s is already present on attribute %s for entity %s"
        }
        "commands.attribute.failed.no_modifier" => "Attribute %s for entity %s has no modifier %s",
        "commands.dialog.clear.single" => "Cleared dialog for %s",
        "commands.dialog.clear.multiple" => "Cleared dialog for %s players",
        "commands.dialog.show.single" => "Showed dialog to %s",
        "commands.dialog.show.multiple" => "Showed dialog to %s players",
        "commands.posteffect.add.success.single" => "Added post effect %s to %s",
        "commands.posteffect.add.success.multiple" => "Added post effect %s to %s players",
        "commands.posteffect.remove.success.single" => "Removed post effect %s from %s",
        "commands.posteffect.remove.success.multiple" => "Removed post effect %s from %s players",
        "commands.posteffect.clear.success.single" => "Cleared post effects for %s",
        "commands.posteffect.clear.success.multiple" => "Cleared post effects for %s players",
        "commands.posteffect.list.empty" => "%s has no post effects",
        "commands.posteffect.list.success" => "%s has %s post effects: %s",
        "commands.posteffect.add.failed" => {
            "Nothing changed. The targets already have this post effect"
        }
        "commands.posteffect.remove.failed" => {
            "Nothing changed. The targets do not have this post effect"
        }
        "commands.posteffect.clear.failed" => "Nothing changed. The targets have no post effects",
        "commands.spectate.success.started" => "Now spectating %s",
        "commands.spectate.success.stopped" => "No longer spectating an entity",
        "commands.spectate.not_spectator" => "%s is not in spectator mode",
        "commands.spectate.self" => "You cannot spectate yourself",
        "commands.spectate.cannot_spectate" => "You cannot spectate %s",
        "argument.entity.toomany" => {
            "Only one entity is allowed, but the provided selector allows more than one"
        }
        "argument.entity.notfound.player" => "No player was found",
        "argument.player.unknown" => "That player does not exist",
        "argument.anchor.invalid" => "Invalid entity anchor position %s",
        "commands.ban.failed" => "Nothing changed. The player is already banned",
        "commands.ban.success" => "Banned %s: %s",
        "commands.banip.failed" => "Nothing changed. That IP is already banned",
        "commands.banip.invalid" => "Invalid IP address or unknown player",
        "commands.banip.success" => "Banned IP %s: %s",
        "commands.banlist.entry" => "%s was banned by %s: %s",
        "commands.banlist.entry.unknown" => "(Unknown)",
        "commands.banlist.list" => "There are %s ban(s):",
        "commands.banlist.none" => "There are no bans",
        "commands.deop.failed" => "Nothing changed. The player is not an operator",
        "commands.deop.success" => "Made %s no longer a server operator",
        "commands.kick.success" => "Kicked %s: %s",
        "commands.kill.success.multiple" => "Killed %s entities",
        "commands.kill.success.single" => "Killed %s",
        "commands.message.display.incoming" => "%s whispers to you: %s",
        "commands.message.display.outgoing" => "You whisper to %s: %s",
        "commands.op.failed" => "Nothing changed. The player is already an operator",
        "commands.op.success" => "Made %s a server operator",
        "commands.pardon.failed" => "Nothing changed. The player isn't banned",
        "commands.pardon.success" => "Unbanned %s",
        "commands.pardonip.failed" => "Nothing changed. That IP isn't banned",
        "commands.pardonip.invalid" => "Invalid IP address",
        "commands.pardonip.success" => "Unbanned IP %s",
        "commands.save.alreadyOff" => "Saving is already turned off",
        "commands.save.alreadyOn" => "Saving is already turned on",
        "commands.save.disabled" => "Automatic saving is now disabled",
        "commands.save.enabled" => "Automatic saving is now enabled",
        "commands.save.failed" => "Unable to save the game (is there enough disk space?)",
        "commands.save.saving" => "Saving the game (this may take a moment!)",
        "commands.save.success" => "Saved the game",
        "commands.stop.stopping" => "Stopping the server",
        "commands.teleport.invalidPosition" => "Invalid position for teleport",
        "commands.teleport.success.entity.multiple" => "Teleported %s entities to %s",
        "commands.teleport.success.entity.single" => "Teleported %s to %s",
        "commands.teleport.success.location.multiple" => "Teleported %s entities to %s, %s, %s",
        "commands.teleport.success.location.single" => "Teleported %s to %s, %s, %s",
        "commands.whitelist.add.failed" => "Player is already whitelisted",
        "commands.whitelist.add.success" => "Added %s to the whitelist",
        "commands.whitelist.alreadyOff" => "Whitelist is already turned off",
        "commands.whitelist.alreadyOn" => "Whitelist is already turned on",
        "commands.whitelist.disabled" => "Whitelist is now turned off",
        "commands.whitelist.enabled" => "Whitelist is now turned on",
        "commands.whitelist.list" => "There are %s whitelisted player(s): %s",
        "commands.whitelist.none" => "There are no whitelisted players",
        "commands.whitelist.reloaded" => "Reloaded the whitelist",
        "commands.whitelist.remove.failed" => "Player is not whitelisted",
        "commands.whitelist.remove.success" => "Removed %s from the whitelist",
        "commands.effect.clear.everything.failed" => "Target has no effects to remove",
        "commands.effect.clear.everything.success.multiple" => {
            "Removed every effect from %s targets"
        }
        "commands.effect.clear.everything.success.single" => "Removed every effect from %s",
        "commands.effect.clear.specific.failed" => "Target doesn't have the requested effect",
        "commands.effect.clear.specific.success.multiple" => "Removed effect %s from %s targets",
        "commands.effect.clear.specific.success.single" => "Removed effect %s from %s",
        "commands.effect.give.failed" => {
            "Unable to apply this effect (target is either immune to effects, or has something stronger)"
        }
        "commands.effect.give.success.multiple" => "Applied effect %s to %s targets",
        "commands.effect.give.success.single" => "Applied effect %s to %s",
        "commands.enchant.failed" => {
            "Nothing changed. Targets either have no item in their hands or the enchantment could not be applied"
        }
        "commands.enchant.failed.entity" => "%s is not a valid entity for this command",
        "commands.enchant.failed.incompatible" => "%s cannot support that enchantment",
        "commands.enchant.failed.itemless" => "%s is not holding any item",
        "commands.enchant.failed.level" => {
            "%s is higher than the maximum level of %s supported by that enchantment"
        }
        "commands.enchant.success.multiple" => "Applied enchantment %s to %s entities",
        "commands.enchant.success.single" => "Applied enchantment %s to %s's item",
        "commands.title.cleared.multiple" => "Cleared titles for %s players",
        "commands.title.cleared.single" => "Cleared titles for %s",
        "commands.title.reset.multiple" => "Reset title options for %s players",
        "commands.title.reset.single" => "Reset title options for %s",
        "commands.title.show.actionbar.multiple" => "Showing new actionbar title for %s players",
        "commands.title.show.actionbar.single" => "Showing new actionbar title for %s",
        "commands.title.show.subtitle.multiple" => "Showing new subtitle for %s players",
        "commands.title.show.subtitle.single" => "Showing new subtitle for %s",
        "commands.title.show.title.multiple" => "Showing new title for %s players",
        "commands.title.show.title.single" => "Showing new title for %s",
        "commands.title.times.multiple" => "Changed title display times for %s players",
        "commands.title.times.single" => "Changed title display times for %s",
        "commands.playsound.failed" => "The sound is too far away to be heard",
        "commands.playsound.success.multiple" => "Played sound %s to %s players",
        "commands.playsound.success.single" => "Played sound %s to %s",
        "commands.stopsound.success.source.any" => "Stopped all '%s' sounds",
        "commands.stopsound.success.source.sound" => "Stopped sound '%s' on source '%s'",
        "commands.stopsound.success.sourceless.any" => "Stopped all sounds",
        "commands.stopsound.success.sourceless.sound" => "Stopped sound '%s'",
        "commands.particle.failed" => "The particle was not visible for anybody",
        "commands.particle.success" => "Displaying particle %s",
        "commands.spawnpoint.success.multiple" => {
            "Set spawn point to %s, %s, %s [%s] in %s for %s players"
        }
        "commands.spawnpoint.success.multiple.new" => {
            "Set spawn point to %s, %s, %s [%s, %s] in %s for %s players"
        }
        "commands.spawnpoint.success.single" => "Set spawn point to %s, %s, %s [%s] in %s for %s",
        "commands.spawnpoint.success.single.new" => {
            "Set spawn point to %s, %s, %s [%s, %s] in %s for %s"
        }
        "commands.setworldspawn.failure.not_overworld" => {
            "Can only set the world spawn for the Overworld"
        }
        "commands.setworldspawn.success" => "Set the world spawn point to %s, %s, %s [%s]",
        "commands.setworldspawn.success.new" => {
            "Set the world spawn point to %s, %s, %s [%s, %s] in %s"
        }
        "commands.worldborder.center.failed" => {
            "Nothing changed. The world border is already centered there"
        }
        "commands.worldborder.center.success" => "Set the center of the world border to %s, %s",
        "commands.worldborder.damage.amount.failed" => {
            "Nothing changed. The world border damage is already that amount"
        }
        "commands.worldborder.damage.amount.success" => {
            "Set the world border damage to %s per block each second"
        }
        "commands.worldborder.damage.buffer.failed" => {
            "Nothing changed. The world border damage buffer is already that distance"
        }
        "commands.worldborder.damage.buffer.success" => {
            "Set the world border damage buffer to %s block(s)"
        }
        "commands.worldborder.get" => "The world border is currently %s block(s) wide",
        "commands.worldborder.set.failed.big" => {
            "World border cannot be bigger than %s blocks wide"
        }
        "commands.worldborder.set.failed.far" => {
            "World border cannot be further out than %s blocks"
        }
        "commands.worldborder.set.failed.nochange" => {
            "Nothing changed. The world border is already that size"
        }
        "commands.worldborder.set.failed.small" => {
            "World border cannot be smaller than 1 block wide"
        }
        "commands.worldborder.set.grow" => {
            "Growing the world border to %s block(s) wide over %s second(s)"
        }
        "commands.worldborder.set.immediate" => "Set the world border to %s block(s) wide",
        "commands.worldborder.set.shrink" => {
            "Shrinking the world border to %s block(s) wide over %s second(s)"
        }
        "commands.worldborder.warning.distance.failed" => {
            "Nothing changed. The world border warning is already that distance"
        }
        "commands.worldborder.warning.distance.success" => {
            "Set the world border warning distance to %s block(s)"
        }
        "commands.worldborder.warning.time.failed" => {
            "Nothing changed. The world border warning is already that amount of time"
        }
        "commands.worldborder.warning.time.success" => {
            "Set the world border warning time to %s second(s)"
        }
        "commands.data.block.get" => "%s on block %s, %s, %s after scale factor of %s is %s",
        "commands.data.block.invalid" => "The target block is not a block entity",
        "commands.data.block.modified" => "Modified block data of %s, %s, %s",
        "commands.data.block.query" => "%s, %s, %s has the following block data: %s",
        "commands.data.entity.get" => "%s on %s after scale factor of %s is %s",
        "commands.data.entity.invalid" => "Unable to modify player data",
        "commands.data.entity.modified" => "Modified entity data of %s",
        "commands.data.entity.query" => "%s has the following entity data: %s",
        "commands.data.get.multiple" => "This argument accepts a single NBT value",
        "commands.data.get.unknown" => "Can't get %s; tag doesn't exist",
        "commands.data.merge.failed" => {
            "Nothing changed. The specified properties already have these values"
        }
        "commands.data.storage.get" => "%s in storage %s after scale factor of %s is %s",
        "commands.data.storage.modified" => "Modified storage %s",
        "commands.data.storage.query" => "Storage %s has the following contents: %s",
        "commands.bossbar.create.success" => "Created custom bossbar %s",
        "commands.bossbar.get.max" => "Custom bossbar %s has a maximum of %s",
        "commands.bossbar.get.players.none" => "Custom bossbar %s has no players currently online",
        "commands.bossbar.get.players.some" => {
            "Custom bossbar %s has %s player(s) currently online: %s"
        }
        "commands.bossbar.get.value" => "Custom bossbar %s has a value of %s",
        "commands.bossbar.get.visible.hidden" => "Custom bossbar %s is currently hidden",
        "commands.bossbar.get.visible.visible" => "Custom bossbar %s is currently visible",
        "commands.bossbar.list.bars.none" => "There are no custom bossbars active",
        "commands.bossbar.list.bars.some" => "There are %s custom bossbar(s) active: %s",
        "commands.bossbar.remove.success" => "Removed custom bossbar %s",
        "commands.bossbar.set.color.success" => "Custom bossbar %s has changed color",
        "commands.bossbar.set.max.success" => "Custom bossbar %s has changed maximum to %s",
        "commands.bossbar.set.name.success" => "Custom bossbar %s has been renamed",
        "commands.bossbar.set.players.success.none" => {
            "Custom bossbar %s no longer has any players"
        }
        "commands.bossbar.set.players.success.some" => "Custom bossbar %s now has %s player(s): %s",
        "commands.bossbar.set.style.success" => "Custom bossbar %s has changed style",
        "commands.bossbar.set.value.success" => "Custom bossbar %s has changed value to %s",
        "commands.bossbar.set.visible.success.hidden" => "Custom bossbar %s is now hidden",
        "commands.bossbar.set.visible.success.visible" => "Custom bossbar %s is now visible",
        "commands.scoreboard.objectives.add.success" => "Created new objective %s",
        "commands.scoreboard.objectives.display.cleared" => {
            "Cleared any objectives in display slot %s"
        }
        "commands.scoreboard.objectives.display.set" => "Set display slot %s to show objective %s",
        "commands.scoreboard.objectives.list.empty" => "There are no objectives",
        "commands.scoreboard.objectives.list.success" => "There are %s objective(s): %s",
        "commands.scoreboard.objectives.modify.displayname" => {
            "Changed the display name of %s to %s"
        }
        "commands.scoreboard.objectives.modify.rendertype" => {
            "Changed the render type of objective %s"
        }
        "commands.scoreboard.objectives.remove.success" => "Removed objective %s",
        "commands.scoreboard.players.add.success.multiple" => "Added %s to %s for %s entities",
        "commands.scoreboard.players.add.success.single" => "Added %s to %s for %s (now %s)",
        "commands.scoreboard.players.enable.success.multiple" => {
            "Enabled trigger %s for %s entities"
        }
        "commands.scoreboard.players.enable.success.single" => "Enabled trigger %s for %s",
        "commands.scoreboard.players.get.success" => "%s has %s %s",
        "commands.scoreboard.players.list.empty" => "There are no tracked entities",
        "commands.scoreboard.players.list.entity.empty" => "%s has no scores to show",
        "commands.scoreboard.players.list.entity.entry" => "%s: %s",
        "commands.scoreboard.players.list.entity.success" => "%s has %s score(s):",
        "commands.scoreboard.players.list.success" => "There are %s tracked entity/entities: %s",
        "commands.scoreboard.players.operation.success.multiple" => "Updated %s for %s entities",
        "commands.scoreboard.players.operation.success.single" => "Set %s for %s to %s",
        "commands.scoreboard.players.remove.success.multiple" => {
            "Removed %s from %s for %s entities"
        }
        "commands.scoreboard.players.remove.success.single" => "Removed %s from %s for %s (now %s)",
        "commands.scoreboard.players.reset.all.multiple" => "Reset all scores for %s entities",
        "commands.scoreboard.players.reset.all.single" => "Reset all scores for %s",
        "commands.scoreboard.players.reset.specific.multiple" => "Reset %s for %s entities",
        "commands.scoreboard.players.reset.specific.single" => "Reset %s for %s",
        "commands.scoreboard.players.set.success.multiple" => "Set %s for %s entities to %s",
        "commands.scoreboard.players.set.success.single" => "Set %s for %s to %s",
        "commands.tag.add.success.multiple" => "Added tag '%s' to %s entities",
        "commands.tag.add.success.single" => "Added tag '%s' to %s",
        "commands.tag.list.multiple.empty" => "There are no tags on the %s entities",
        "commands.tag.list.multiple.success" => "The %s entities have %s total tag(s): %s",
        "commands.tag.list.single.empty" => "%s has no tags",
        "commands.tag.list.single.success" => "%s has %s tag(s): %s",
        "commands.tag.remove.success.multiple" => "Removed tag '%s' from %s entities",
        "commands.tag.remove.success.single" => "Removed tag '%s' from %s",
        "commands.team.add.success" => "Created team %s",
        "commands.team.empty.success" => "Removed %s member(s) from team %s",
        "commands.team.join.success.multiple" => "Added %s members to team %s",
        "commands.team.join.success.single" => "Added %s to team %s",
        "commands.team.leave.success.multiple" => "Removed %s members from any team",
        "commands.team.leave.success.single" => "Removed %s from any team",
        "commands.team.list.members.empty" => "There are no members on team %s",
        "commands.team.list.members.success" => "Team %s has %s member(s): %s",
        "commands.team.list.teams.empty" => "There are no teams",
        "commands.team.list.teams.success" => "There are %s team(s): %s",
        "commands.team.option.collisionRule.success" => "Collision rule for team %s is now \"%s\"",
        "commands.team.option.color.clear.success" => "Cleared the color for team %s",
        "commands.team.option.color.success" => "Updated the color for team %s to %s",
        "commands.team.option.deathMessageVisibility.success" => {
            "Death message visibility for team %s is now \"%s\""
        }
        "commands.team.option.friendlyfire.disabled" => "Disabled friendly fire for team %s",
        "commands.team.option.friendlyfire.enabled" => "Enabled friendly fire for team %s",
        "commands.team.option.name.success" => "Updated the name of team %s",
        "commands.team.option.nametagVisibility.success" => {
            "Nametag visibility for team %s is now \"%s\""
        }
        "commands.team.option.prefix.success" => "Team prefix set to %s",
        "commands.team.option.seeFriendlyInvisibles.disabled" => {
            "Team %s can no longer see invisible teammates"
        }
        "commands.team.option.seeFriendlyInvisibles.enabled" => {
            "Team %s can now see invisible teammates"
        }
        "commands.team.option.suffix.success" => "Team suffix set to %s",
        "commands.team.remove.success" => "Removed team %s",
        "commands.function.unknown" => "Unknown function %s",
        "commands.function.return.not_in_function" => {
            "This command can only be run from inside a function"
        }
        _ => return None,
    })
}

/// Renders `key` with `args`, substituting `%s` placeholders in order.
///
/// Unknown keys render as the key itself followed by the arguments; every key this server
/// emits is in the table above.
pub fn render(key: &str, args: &[&str]) -> String {
    let Some(template) = template(key) else {
        if args.is_empty() {
            return key.to_owned();
        }
        return format!("{key}: {}", args.join(", "));
    };
    let mut parts = template.split("%s");
    let mut text = parts.next().unwrap_or_default().to_owned();
    for (arg, part) in args.iter().zip(parts) {
        text.push_str(arg);
        text.push_str(part);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_vanilla_feedback() {
        assert_eq!(
            render("commands.op.success", &["Alice"]),
            "Made Alice a server operator"
        );
        assert_eq!(
            render(
                "commands.teleport.success.location.single",
                &["Alice", "10.5", "70.0", "20.5"]
            ),
            "Teleported Alice to 10.5, 70.0, 20.5"
        );
        assert_eq!(
            render("commands.message.display.incoming", &["Server", "hi"]),
            "Server whispers to you: hi"
        );
        assert_eq!(
            render("commands.banlist.entry", &["Bob", "Server", "griefing"]),
            "Bob was banned by Server: griefing"
        );
        assert_eq!(render("commands.banlist.none", &[]), "There are no bans");
        assert_eq!(
            render("argument.entity.notfound.player", &[]),
            "No player was found"
        );
        assert_eq!(
            render("argument.anchor.invalid", &["head"]),
            "Invalid entity anchor position head"
        );
        assert_eq!(render("unknown.key", &[]), "unknown.key");
    }
}
