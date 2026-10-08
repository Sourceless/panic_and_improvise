Feature: Firing the gun

  Background:
    Given I am in test room 1
    And I have the smg

  Scenario: Firing plays the shot sound on the audio device
    Given the shot sound is loaded
    When I fire once
    Then the shot sound plays on the audio device

  Scenario: Firing with the cursor released does nothing
    Given the cursor is released
    When I hold fire for 0.5 seconds
    Then no shot is fired

  Scenario: Holding fire is rate limited
    When I hold fire for 1 second
    Then the gun fired between 7 and 9 shots

  Scenario: A shot hits the target dummy
    When I fire once
    Then the target dummy has taken 25 damage

  Scenario: The target dummy respawns after being destroyed
    When I fire single shots until the target dummy is down
    Then the target dummy respawns within 3 seconds

  Scenario: Firing kicks the view upward
    When I fire once
    Then the view has kicked upward

  Scenario: A burst climbs, and then settles most of the way back
    When I hold fire for 1 second
    Then the view has climbed noticeably
    And the view has come most of the way back down
